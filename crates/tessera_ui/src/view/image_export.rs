//! File ▸ Export image…: pages, spreads or the selection as PNG, JPEG, TIFF
//! or WebP.
//!
//! The pictures are the printed page drawn at a resolution — see
//! `tessera_pdf::raster` — so a picture of a page is what a proof of it would
//! show, without the canvas's frame edges and guides. The choices follow
//! InDesign's Export JPEG and PNG dialogs, with TIFF and WebP beside them:
//! which pages or the selection, pages or spreads; the resolution, the
//! colours, the quality and paper; and whether to carry a profile, take in
//! the bleed and open the result. What is being made is said in a sentence
//! before anything is written.

use std::path::{Path, PathBuf};

use egui::Ui;
use tessera_document::intent::OutputIntent;
use tessera_layout::ResolvedDocument;
use tessera_pdf::raster::{self, Colour, Format, ImageOptions};

use crate::app::TesseraApp;
use crate::theme::Theme;
use crate::view::style_ui::{self, Segment};

/// What an image export makes, as last chosen: the pictures themselves, and
/// the two choices that are about the export rather than the pictures.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Choices {
    /// Flattened, so a preferences file from before the other two existed
    /// still reads its picture choices.
    #[serde(flatten)]
    pub picture: ImageOptions,
    /// A spread as one picture rather than a picture a page.
    pub spreads: bool,
    /// Open the picture when it is written — or, for several, show them.
    pub open_after: bool,
}

/// Which part of the document becomes pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Which {
    #[default]
    All,
    /// The pages the range names.
    Range,
    /// What is selected, cut out to what it paints.
    Selection,
}

/// The dialog's own state. What the pictures are like is a preference,
/// remembered from one export to the next; which pages is a question about
/// this document, asked each time.
#[derive(Debug, Clone)]
pub struct ImageExportWindow {
    pub open: bool,
    pub which: Which,
    /// Pages as InDesign writes them: `1-3, 6, 9-`.
    pub range: String,
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
    range: String,
    choices: Choices,
    selection: Vec<tessera_document::ids::FrameId>,
}

impl Default for ImageExportWindow {
    fn default() -> Self {
        ImageExportWindow {
            open: false,
            which: Which::All,
            range: "1".to_string(),
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
    window.range = page.to_string();
}

/// The document's spreads, each as the indices of its pages in reading
/// order.
pub fn spread_groups(state: &TesseraApp) -> Vec<Vec<usize>> {
    let doc = state.active().document();
    let order: Vec<_> = doc.page_ids().collect();
    doc.spread_ids()
        .map(|spread| {
            doc.pages_of(spread)
                .into_iter()
                .filter_map(|page| order.iter().position(|p| *p == page))
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

/// What an export would make: one resolved page for each picture, and the
/// one-based page numbers each is named by — empty for a selection, which is
/// one picture named by the file itself.
pub fn chosen(
    state: &TesseraApp,
    resolved: &ResolvedDocument,
    which: Which,
    range: &str,
    spreads: bool,
) -> Result<(ResolvedDocument, Vec<Vec<usize>>), String> {
    use tessera_pdf::pages;

    match which {
        Which::All | Which::Range => {
            let count = resolved.pages.len();
            let chosen = if which == Which::All {
                (0..count).collect()
            } else {
                pages::parse_range(range, count)?
            };
            let spread_list = spreads.then(|| spread_groups(state));
            let groups = pages::groups(&chosen, spread_list.as_deref());
            if groups.is_empty() {
                return Err("There are no pages to export.".to_string());
            }
            let numbers = groups
                .iter()
                .map(|g| g.iter().map(|i| i + 1).collect())
                .collect();
            Ok((pages::assemble(resolved, &groups), numbers))
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
                .map(|cut| (cut, vec![Vec::new()]))
                .ok_or_else(|| "What is selected paints nothing to picture.".to_string())
        }
    }
}

/// The files `numbers` pictures are written to, given the name chosen: that
/// name itself for one picture, and the name with each picture's page
/// numbers after it for several — padded, so a folder of them sorts in page
/// order, and joined for a spread: `Brochure-02-03.png`.
pub fn file_names(chosen: &Path, numbers: &[Vec<usize>], format: Format) -> Vec<PathBuf> {
    let with_extension = |path: &Path| {
        let mut path = path.to_path_buf();
        let fits = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            format
                .extensions()
                .iter()
                .any(|known| e.eq_ignore_ascii_case(known))
        });
        // Another picture format's extension is replaced; anything else
        // after a dot is part of the name, and the extension goes after it.
        let other_picture = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            Format::ALL
                .iter()
                .flat_map(|f| f.extensions())
                .any(|known| e.eq_ignore_ascii_case(known))
        });
        if other_picture && !fits {
            path.set_extension(format.extension());
        } else if !fits {
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
    if numbers.len() <= 1 {
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
        .map(|(i, group)| {
            let label = if group.is_empty() {
                format!("{:0widest$}", i + 1)
            } else {
                group
                    .iter()
                    .map(|n| format!("{n:0widest$}"))
                    .collect::<Vec<_>>()
                    .join("-")
            };
            base.with_file_name(format!("{stem}-{label}.{extension}"))
        })
        .collect()
}

/// The press a CMYK picture is converted for: the document's own when it
/// names a CMYK one, else the coated press a new print document starts
/// with, from the profiles Tessera ships. `None` when neither is there.
pub fn press(state: &TesseraApp) -> Option<OutputIntent> {
    use tessera_color::managed::OutputProfile;

    let is_cmyk = |intent: &OutputIntent| {
        OutputProfile::from_bytes(intent.profile.clone()).is_ok_and(|p| p.space() == "CMYK")
    };
    if let Some(intent) = state.active().document().output_intent.clone()
        && is_cmyk(&intent)
    {
        return Some(intent);
    }
    let shipped = tessera_color::profiles::bundled();
    let usual = shipped
        .iter()
        .filter(|b| b.space == "CMYK")
        .find(|b| b.path.file_name().is_some_and(|n| n == "CGATS21_CRPC6.icc"))
        .or_else(|| shipped.iter().find(|b| b.space == "CMYK"))?;
    let bytes = std::fs::read(&usual.path).ok()?;
    OutputProfile::from_bytes(bytes.clone()).ok()?;
    Some(OutputIntent {
        // The manifest's name, which is a press; the profile's own
        // description of itself is sometimes only its file name.
        description: usual.name.clone(),
        profile: bytes,
        rendering: Default::default(),
    })
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.image_export.open {
        return;
    }
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("export-images"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 470.0));
            ui.heading("Export image");
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
    let mut choices = state.prefs.image_export;
    let has_selection =
        !state.active().selection.as_slice().is_empty() && state.editing_master.is_none();
    let mut window = state.image_export.clone();
    if window.which == Which::Selection && !has_selection {
        window.which = Which::All;
    }

    style_ui::card(ui, Some("Export"), |ui| {
        row(ui, "Format", |ui| {
            let formats: Vec<_> = Format::ALL
                .iter()
                .map(|f| (Segment::Text(f.label()), *f))
                .collect();
            style_ui::segmented(ui, "Format", &mut choices.picture.format, &formats, false);
        });
        row(ui, "Pages", |ui| {
            let mut which = vec![
                (Segment::Text("All"), Which::All),
                (Segment::Text("Range"), Which::Range),
            ];
            if has_selection {
                which.push((Segment::Text("Selection"), Which::Selection));
            }
            style_ui::segmented(ui, "Pages", &mut window.which, &which, false);
        });
        if window.which == Which::Range {
            row(ui, "", |ui| {
                crate::icons::speak_as(
                    ui.add(
                        egui::TextEdit::singleline(&mut window.range)
                            .hint_text("1-3, 6, 9-")
                            .desired_width(160.0),
                    ),
                    "Page range",
                );
            });
        }
        if window.which != Which::Selection {
            row(ui, "As", |ui| {
                style_ui::segmented(
                    ui,
                    "As",
                    &mut choices.spreads,
                    &[
                        (Segment::Text("Pages"), false),
                        (Segment::Text("Spreads"), true),
                    ],
                    false,
                );
            });
        }
    });

    let settled = choices.picture.settled();
    style_ui::card(ui, Some("Image"), |ui| {
        let picture = &mut choices.picture;
        row(ui, "Resolution", |ui| {
            for ppi in raster::USUAL_PPI {
                let chosen = (picture.ppi - ppi).abs() < 0.5;
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
                    picture.ppi = ppi;
                }
            }
            crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut picture.ppi)
                        .range(raster::PPI_RANGE)
                        .fixed_decimals(0)
                        .suffix(" ppi"),
                ),
                "Pixels to the inch",
            );
        });
        row(ui, "Colour", |ui| {
            let colours: Vec<_> = picture
                .format
                .colours()
                .iter()
                .map(|c| (Segment::Text(c.label()), *c))
                .collect();
            let mut colour = settled.colour;
            if style_ui::segmented(ui, "Colour", &mut colour, &colours, false) {
                picture.colour = colour;
            }
        });
        if picture.format.is_lossy() {
            row(ui, "Quality", |ui| {
                let levels: Vec<_> = raster::QUALITY_LEVELS
                    .iter()
                    .map(|(name, q)| (Segment::Text(name), *q))
                    .collect();
                style_ui::segmented(ui, "Quality", &mut picture.quality, &levels, false);
                let mut number = f64::from(picture.quality);
                crate::icons::speak_as(
                    ui.add(
                        egui::DragValue::new(&mut number)
                            .range(1.0..=100.0)
                            .fixed_decimals(0),
                    ),
                    "Quality number",
                );
                picture.quality = number.round() as u8;
            });
            row(ui, "Method", |ui| {
                style_ui::segmented(
                    ui,
                    "Method",
                    &mut picture.progressive,
                    &[
                        (Segment::Text("Baseline"), false),
                        (Segment::Text("Progressive"), true),
                    ],
                    false,
                );
            });
        }
        let could_be_clear = ImageOptions {
            transparent: true,
            ..settled
        }
        .settled()
        .transparent;
        if could_be_clear {
            row(ui, "Paper", |ui| {
                style_ui::segmented(
                    ui,
                    "Paper",
                    &mut picture.transparent,
                    &[
                        (Segment::Text("White"), false),
                        (Segment::Text("Clear"), true),
                    ],
                    false,
                );
            });
        }
    });

    style_ui::card(ui, Some("Options"), |ui| {
        let grey = settled.colour == Colour::Grey;
        ui.add_enabled_ui(!grey, |ui| {
            ui.checkbox(
                &mut choices.picture.embed_profile,
                "Embed the colour profile",
            )
            .on_disabled_hover_text("A grey picture carries no profile");
        });
        if window.which != Which::Selection {
            ui.checkbox(&mut choices.picture.bleed, "Include the bleed");
        }
        ui.checkbox(&mut choices.open_after, "Open when done");
    });

    state.prefs.image_export = choices;
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
        if ui.add(crate::view::secondary_button("Cancel")).clicked() {
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
        range: window.range.clone(),
        choices: state.prefs.image_export,
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

/// A size in bytes as people say it.
fn megabytes(bytes: usize) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// What an export would make, in a sentence — or why it cannot.
pub fn plan(state: &mut TesseraApp) -> Result<String, String> {
    let choices = state.prefs.image_export;
    let options = choices.picture.settled();
    let window = state.image_export.clone();
    let resolved = state.resolve_uncached();
    let (chosen, numbers) = chosen(
        state,
        &resolved,
        window.which,
        &window.range,
        choices.spreads,
    )?;
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
    let (colours, carried) = match options.colour {
        Colour::Rgb => ("in RGB".to_string(), None),
        Colour::Grey => ("in grey".to_string(), None),
        Colour::Cmyk => {
            let press = press(state).ok_or_else(|| {
                "CMYK needs a press profile: name a CMYK press in Document Setup.".to_string()
            })?;
            let carried = options.embed_profile.then_some(press.profile.len());
            (format!("in CMYK for {}", press.description), carried)
        }
    };
    let kind = options.format.label();
    let count = numbers.len();
    // A selection is one picture, whose sentence says neither.
    let one = if choices.spreads {
        "a spread"
    } else {
        "a page"
    };
    let (w, h) = sizes[0];
    let same = sizes.iter().all(|s| *s == sizes[0]);
    let mut said = match (count, same) {
        (1, _) => format!("One {kind} picture {colours}, {w} × {h} pixels."),
        (_, true) => {
            format!("{count} {kind} pictures {colours}, {w} × {h} pixels each, one {one}.")
        }
        (_, false) => {
            let (w, h) = sizes
                .iter()
                .max_by_key(|(w, h)| w * h)
                .copied()
                .unwrap_or((w, h));
            format!("{count} {kind} pictures {colours}, one {one}, up to {w} × {h} pixels.")
        }
    };
    if let Some(bytes) = carried {
        said.push_str(&format!(
            " {} the press's profile, {}.",
            if count == 1 {
                "It carries"
            } else {
                "Each carries"
            },
            megabytes(bytes)
        ));
    }
    Ok(said)
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

    /// Three pages: the first two a spread, the third alone.
    fn a_spread_and_a_page() -> TesseraApp {
        let mut state = TesseraApp::headless();
        let spread = state
            .active()
            .document()
            .spread_ids()
            .next()
            .expect("a spread");
        // undo-bracketed: a fixture, built before anything is undone.
        state.active_mut().document_mut().add_page_to(spread);
        apply(&mut state, Command::AddPage);
        state
    }

    fn a_box(state: &mut TesseraApp) {
        let page = state.first_page_bounds();
        apply(
            state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: page.x + 10.0,
                y: page.y + 10.0,
                width: 50.0,
                height: 30.0,
            }),
        );
    }

    #[test]
    fn several_pictures_are_numbered_and_one_is_named_as_chosen() {
        let chosen = Path::new("/out/Brochure.png");
        assert_eq!(
            file_names(chosen, &[vec![3]], Format::Png),
            [PathBuf::from("/out/Brochure.png")]
        );
        assert_eq!(
            file_names(chosen, &[vec![9], vec![10], vec![11]], Format::Png),
            [
                PathBuf::from("/out/Brochure-09.png"),
                PathBuf::from("/out/Brochure-10.png"),
                PathBuf::from("/out/Brochure-11.png"),
            ],
            "padded, so they sort in page order"
        );
        assert_eq!(
            file_names(chosen, &[vec![1], vec![2, 3]], Format::Tiff),
            [
                PathBuf::from("/out/Brochure-1.tif"),
                PathBuf::from("/out/Brochure-2-3.tif"),
            ],
            "a spread is named by both its pages, and another format's \
             extension is replaced"
        );
        // The format's own extension, whatever the name was typed with.
        assert_eq!(
            file_names(Path::new("/out/Brochure"), &[Vec::new()], Format::Jpeg),
            [PathBuf::from("/out/Brochure.jpg")]
        );
        for kept in ["/out/cover.JPEG", "/out/cover.tiff", "/out/cover.webp"] {
            let format = match kept.rsplit('.').next() {
                Some("JPEG") => Format::Jpeg,
                Some("tiff") => Format::Tiff,
                _ => Format::WebP,
            };
            assert_eq!(
                file_names(Path::new(kept), &[Vec::new()], format),
                [PathBuf::from(kept)],
                "{kept} is already a {format:?}"
            );
        }
        assert_eq!(
            file_names(Path::new("/out/v1.2"), &[vec![1], vec![2]], Format::Png),
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
                egui::vec2(1200.0, 1100.0),
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
        // egui tells a double click by time, and a test clicks faster than
        // anybody: idle frames first, so two clicks are two clicks.
        for _ in 0..40 {
            draw(ctx, state, Vec::new());
        }
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

    fn has(shown: &[String], name: &str) -> bool {
        shown.iter().any(|n| n == name)
    }

    #[test]
    fn the_dialog_offers_what_the_format_can_do() {
        let mut state = two_pages();
        a_box(&mut state);
        open(&mut state);
        let ctx = a_window();
        let shown = names(&ctx, &mut state);
        for name in ["PNG", "JPEG", "TIFF", "WebP", "All", "Range", "Selection"] {
            assert!(has(&shown, name), "no {name:?} in {shown:?}");
        }
        assert!(
            has(&shown, "Clear") && !has(&shown, "CMYK"),
            "a PNG: clear, no CMYK"
        );
        assert!(
            !has(&shown, "Include the bleed"),
            "a selection has no bleed"
        );
        assert!(!has(&shown, "Spreads"), "nor spreads");

        click(&ctx, &mut state, "TIFF");
        assert_eq!(state.prefs.image_export.picture.format, Format::Tiff);
        assert!(has(&names(&ctx, &mut state), "CMYK"), "a TIFF can be CMYK");

        click(&ctx, &mut state, "JPEG");
        let shown = names(&ctx, &mut state);
        for name in [
            "Low",
            "Medium",
            "High",
            "Maximum",
            "Baseline",
            "Progressive",
            "CMYK",
        ] {
            assert!(has(&shown, name), "no {name:?} for a JPEG in {shown:?}");
        }
        assert!(!has(&shown, "Clear"), "a JPEG cannot be clear");
        click(&ctx, &mut state, "Maximum");
        assert_eq!(state.prefs.image_export.picture.quality, 100);
        click(&ctx, &mut state, "Progressive");
        assert!(state.prefs.image_export.picture.progressive);
        click(&ctx, &mut state, "Grey");
        assert_eq!(state.prefs.image_export.picture.colour, Colour::Grey);

        click(&ctx, &mut state, "WebP");
        let shown = names(&ctx, &mut state);
        assert!(
            !has(&shown, "Quality") && !has(&shown, "Baseline"),
            "WebP is lossless"
        );
        assert!(!has(&shown, "CMYK"));

        click(&ctx, &mut state, "300");
        assert_eq!(state.prefs.image_export.picture.ppi, 300.0);
    }

    #[test]
    fn a_range_is_typed_and_spreads_are_a_choice() {
        let mut state = a_spread_and_a_page();
        open(&mut state);
        let ctx = a_window();
        click(&ctx, &mut state, "Range");
        assert_eq!(state.image_export.which, Which::Range);
        let shown = names(&ctx, &mut state);
        for name in [
            "Page range",
            "Pages",
            "Spreads",
            "Include the bleed",
            "Open when done",
        ] {
            assert!(has(&shown, name), "no {name:?} in {shown:?}");
        }
        click(&ctx, &mut state, "Spreads");
        assert!(state.prefs.image_export.spreads);
        click(&ctx, &mut state, "Include the bleed");
        assert!(state.prefs.image_export.picture.bleed);
        click(&ctx, &mut state, "Open when done");
        assert!(state.prefs.image_export.open_after);
        click(&ctx, &mut state, "Cancel");
        assert!(!state.image_export.open);
    }

    #[test]
    fn with_nothing_selected_the_selection_is_not_offered() {
        let mut state = two_pages();
        open(&mut state);
        state.image_export.which = Which::Selection;
        let ctx = a_window();
        let shown = names(&ctx, &mut state);
        assert!(!has(&shown, "Selection"), "{shown:?}");
        assert_eq!(state.image_export.which, Which::All, "and it is not chosen");
    }

    #[test]
    fn the_dialog_opens_on_the_selection_or_on_the_page_being_worked_on() {
        let mut state = two_pages();
        open(&mut state);
        assert!(state.image_export.open);
        assert_eq!(state.image_export.which, Which::All);
        assert_eq!(state.image_export.range, "1");

        a_box(&mut state);
        open(&mut state);
        assert_eq!(state.image_export.which, Which::Selection);
    }

    #[test]
    fn the_sentence_says_what_will_be_made() {
        let mut state = two_pages();
        state.image_export.which = Which::All;
        state.prefs.image_export.picture.ppi = 72.0;
        let page = state.first_page_bounds();
        let (w, h) = (page.width.round(), page.height.round());
        assert_eq!(
            plan(&mut state),
            Ok(format!(
                "2 PNG pictures in RGB, {w} × {h} pixels each, one a page."
            ))
        );

        state.image_export.which = Which::Range;
        state.image_export.range = "2".into();
        state.prefs.image_export.picture.format = Format::Jpeg;
        state.prefs.image_export.picture.colour = Colour::Grey;
        assert_eq!(
            plan(&mut state),
            Ok(format!("One JPEG picture in grey, {w} × {h} pixels."))
        );

        state.image_export.range = "5".into();
        assert_eq!(
            plan(&mut state),
            Err("There is no page 5: the document has 2 pages.".to_string())
        );

        state.image_export.range = "1-2".into();
        state.prefs.image_export.picture.ppi = 2400.0;
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
    fn spreads_are_one_picture_each() {
        let mut state = a_spread_and_a_page();
        let mut sizes: Vec<usize> = spread_groups(&state).iter().map(Vec::len).collect();
        sizes.sort_unstable();
        assert_eq!(sizes, [1, 2], "a spread of two and a page alone");
        state.image_export.which = Which::All;
        state.prefs.image_export.picture.ppi = 72.0;
        state.prefs.image_export.spreads = true;
        let page = state.first_page_bounds();
        let (w, h) = ((page.width * 2.0).round(), page.height.round());
        assert_eq!(
            plan(&mut state),
            Ok(format!(
                "2 PNG pictures in RGB, one a spread, up to {w} × {h} pixels."
            ))
        );
        state.image_export.which = Which::Range;
        state.image_export.range = "2".into();
        assert_eq!(
            plan(&mut state),
            Ok(format!("One PNG picture in RGB, {w} × {h} pixels.")),
            "a page brings its whole spread"
        );
    }

    #[test]
    fn a_cmyk_picture_names_its_press_and_the_profile_it_carries() {
        let mut state = two_pages();
        // undo-bracketed: a fixture, built before anything is undone.
        state.active_mut().document_mut().output_intent =
            Some(tessera_document::intent::OutputIntent {
                description: "Coated press".into(),
                profile: include_bytes!("../../../../assets/profiles/CGATS21_CRPC6.icc").to_vec(),
                rendering: Default::default(),
            });
        state.image_export.which = Which::Range;
        state.image_export.range = "1".into();
        state.prefs.image_export.picture = ImageOptions {
            format: Format::Tiff,
            colour: Colour::Cmyk,
            ppi: 72.0,
            ..ImageOptions::default()
        };
        let said = plan(&mut state).expect("a plan");
        assert!(
            said.starts_with("One TIFF picture in CMYK for Coated press"),
            "{said}"
        );
        assert!(
            said.ends_with("It carries the press's profile, 3.3 MB."),
            "{said}"
        );

        state.prefs.image_export.picture.embed_profile = false;
        let said = plan(&mut state).expect("a plan");
        assert!(!said.contains("carries"), "{said}");

        // A PNG cannot be CMYK: the choice is settled to RGB.
        state.prefs.image_export.picture.format = Format::Png;
        assert!(plan(&mut state).expect("a plan").contains("in RGB"));
    }

    #[test]
    fn with_no_cmyk_press_named_cmyk_is_for_the_coated_press_tessera_ships() {
        // As a new print document starts: the coated press, CRPC6.
        let mut state = two_pages();
        assert!(state.active().document().output_intent.is_none());
        let press = press(&state).expect("the bundled profiles are in the checkout");
        assert_eq!(
            press.profile,
            include_bytes!("../../../../assets/profiles/CGATS21_CRPC6.icc").to_vec()
        );
        state.image_export.which = Which::All;
        state.prefs.image_export.picture = ImageOptions {
            format: Format::Jpeg,
            colour: Colour::Cmyk,
            ppi: 72.0,
            ..ImageOptions::default()
        };
        let said = plan(&mut state).expect("a plan");
        assert!(
            said.starts_with(&format!(
                "2 JPEG pictures in CMYK for {}",
                press.description
            )),
            "{said}"
        );
        assert!(
            said.ends_with("Each carries the press's profile, 3.3 MB."),
            "{said}"
        );

        // An RGB press named is not a CMYK one: the shipped press stands in.
        let screen = tessera_color::managed::OutputProfile::screen().expect("sRGB");
        // undo-bracketed: a fixture, built before anything is undone.
        state.active_mut().document_mut().output_intent =
            Some(tessera_document::intent::OutputIntent {
                description: "Screen".into(),
                profile: screen.bytes().to_vec(),
                rendering: Default::default(),
            });
        assert_eq!(super::press(&state).map(|p| p.profile), Some(press.profile));
    }

    #[test]
    fn a_selection_is_one_picture_of_what_it_paints() {
        let mut state = TesseraApp::headless();
        a_box(&mut state);
        state.image_export.which = Which::Selection;
        state.prefs.image_export.picture.ppi = 72.0;
        // A new rectangle's hairline is a point wide on its edge: half of it
        // outside, so the picture is a point bigger than the box.
        assert_eq!(
            plan(&mut state),
            Ok("One PNG picture in RGB, 51 × 31 pixels.".into())
        );

        state.active_mut().selection.clear();
        assert_eq!(plan(&mut state), Err("Nothing is selected.".into()));
    }

    #[test]
    fn the_sentence_is_worked_out_again_only_when_something_changed() {
        let mut state = two_pages();
        state.prefs.image_export.picture.ppi = 72.0;
        let first = planned(&mut state);
        // A stale answer planted where the worked-out one is kept: asked
        // again with nothing changed, the kept one is what comes back.
        if let Some((_, said)) = state.image_export.planned.as_mut() {
            *said = Ok("kept".to_string());
        }
        assert_eq!(planned(&mut state), Ok("kept".to_string()));
        state.prefs.image_export.picture.ppi = 144.0;
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
}
