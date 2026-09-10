//! The New Document dialog.
//!
//! Tessera used to open with a document already made. That is convenient for
//! about ten seconds and wrong for everything after: a page size, a bleed and a
//! press are decisions a job is built on, and changing them later means moving
//! everything that was laid out against the old ones. Every layout tool asks
//! first for that reason.
//!
//! ## The colour question is RGB or CMYK, not which profile
//!
//! "Which ICC profile" is a question with about nine answers, seven of which
//! differ in ways only a printer can explain. Somebody starting a job knows
//! whether it is going on screen or on a press; they do not yet know whether
//! their printer runs CRPC3 or CRPC6, and asking them to guess makes the answer
//! worse than a sensible default. The press can be changed in document setup
//! once there is a printer to ask.
//!
//! ## Everything has a default, and the defaults are somebody's job
//!
//! A4, facing pages, sensible margins and a 3mm bleed. Somebody who presses
//! Return without reading gets a document a printer would accept, which is the
//! whole test of a default.

use egui::Ui;

use crate::app::TesseraApp;
use crate::theme::Theme;
use tessera_document::nodes::{Orientation, PagePreset};

/// The bleed a commercial printer expects, in millimetres.
///
/// Three is the European convention and an eighth of an inch (3.175mm) the
/// American one; they are close enough that three works for both, and a
/// document with *some* bleed is immeasurably better than one with none.
const USUAL_BLEED_MM: f64 = 3.0;

/// The margin a page starts with, in millimetres.
const USUAL_MARGIN_MM: f64 = 12.7;

const MM: f64 = 72.0 / 25.4;

/// Which inks the job is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Screens, and anything that ends up on one.
    Screen,
    /// A press.
    Print,
}

impl Intent {
    fn label(self) -> &'static str {
        match self {
            Intent::Screen => "RGB — screen, web, presentation",
            Intent::Print => "CMYK — commercial print",
        }
    }

    /// The bundled profile this intent starts with.
    ///
    /// **A default, not a decision.** CRPC6 is a premium coated sheet-fed
    /// condition — the one a general commercial job is most likely to land on,
    /// and the one whose gamut is widest among the coated conditions, so a
    /// document proofed against it is not flattered by a press with less.
    /// Document setup is where it gets changed once there is a printer to ask.
    fn profile(self) -> Option<&'static str> {
        match self {
            Intent::Screen => None,
            Intent::Print => Some("CGATS21_CRPC6.icc"),
        }
    }
}

/// What the dialog is currently asking for.
#[derive(Debug, Clone)]
pub struct NewDocument {
    pub open: bool,
    pub preset: Option<PagePreset>,
    pub width: f64,
    pub height: f64,
    pub orientation: Orientation,
    pub facing_pages: bool,
    pub pages: u32,
    pub margin: f64,
    pub bleed: f64,
    pub intent: Intent,
    /// Effective resolution to warn below, in pixels per inch.
    pub minimum_ppi: f64,
    /// Whether the page behind the dialog shows what it will make.
    ///
    /// On, as InDesign has it. A page size is hard to picture from two numbers
    /// and easy to recognise on sight, and the commonest mistake this dialog can
    /// let through \— landscape when portrait was meant, or a trim nothing will
    /// fit on \— is one nobody makes twice after seeing it.
    pub preview: bool,
}

impl Default for NewDocument {
    fn default() -> Self {
        let (width, height) = PagePreset::A4.size();
        Self {
            open: false,
            preset: Some(PagePreset::A4),
            width,
            height,
            orientation: Orientation::Portrait,
            facing_pages: true,
            pages: 1,
            margin: USUAL_MARGIN_MM * MM,
            bleed: USUAL_BLEED_MM * MM,
            intent: Intent::Print,
            minimum_ppi: 300.0,
            preview: true,
        }
    }
}

impl NewDocument {
    /// The page size as it will actually be made, orientation applied.
    pub fn page(&self) -> (f64, f64) {
        self.orientation.apply(self.width, self.height)
    }

    /// Take a preset, keeping the orientation already chosen.
    fn take(&mut self, preset: PagePreset) {
        let (w, h) = preset.size();
        self.preset = Some(preset);
        self.width = w;
        self.height = h;
    }
}

/// Show the dialog, and make the document if it is accepted.
pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.new_document.open {
        return;
    }

    sync_preview(state);

    let mut settings = state.new_document.clone();
    let mut make = false;
    let mut cancel = false;
    let unit = state.prefs.unit;

    egui::Window::new("New document")
        .collapsible(false)
        .resizable(false)
        .default_width(420.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            body(ui, &mut settings, unit);

            ui.add_space(Theme::SPACE_3);
            ui.horizontal(|ui| {
                make = ui.button("Create").clicked();
                cancel = ui.button("Cancel").clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut settings.preview, "Preview");
                });
            });
        });

    state.new_document = settings;
    if make {
        create(state);
        state.new_document.open = false;
    } else if cancel && state.documents.len() > 1 {
        // Cancel closes the dialog only when there is something behind it.
        // With no document open it would leave an application showing nothing
        // and offering no way back, which is worse than not offering cancel.
        state.new_document.open = false;
    } else if cancel {
        state.status = Some(crate::app::Status::info(
            "Tessera needs a document to work in. Create one, or open an existing file.",
        ));
    }
}

fn body(ui: &mut Ui, settings: &mut NewDocument, unit: tessera_geometry::Unit) {
    use crate::view::panels::{field, measure_bare, pair};

    // The preset names a pair of numbers somebody recognises; the document
    // still stores only a width and a height. "Custom" is not a value — it is
    // what no preset matching looks like.
    let shown = settings
        .preset
        .map_or("Custom", PagePreset::name)
        .to_string();
    let mut chosen = None;
    field(ui, "Size", |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("new-page-preset")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for preset in PagePreset::ALL {
                        let (w, h) = preset.size();
                        if ui
                            .selectable_label(settings.preset == Some(preset), preset.name())
                            // The measurements, so somebody who does not recognise
                            // "Demy octavo" can still tell whether it is the one.
                            .on_hover_text(format!(
                                "{:.1} × {:.1} {}",
                                unit.from_points(w),
                                unit.from_points(h),
                                unit.suffix().trim()
                            ))
                            .clicked()
                        {
                            chosen = Some(preset);
                        }
                    }
                })
                .response,
            "Size",
            egui::WidgetType::ComboBox,
            None,
        );
    });
    if let Some(preset) = chosen {
        settings.take(preset);
    }

    let mut resized = false;
    let (a, b) = pair(
        ui,
        ("Width", |ui: &mut Ui| {
            measure_bare(ui, &mut settings.width, unit)
        }),
        ("Height", |ui: &mut Ui| {
            measure_bare(ui, &mut settings.height, unit)
        }),
    );
    resized |= a || b;
    if resized {
        // Typing a size makes it Custom, unless it happens to be a paper. That
        // is not a special case: it is `matching` answering honestly.
        settings.preset = PagePreset::matching(settings.width, settings.height);
    }

    field(ui, "Orientation", |ui| {
        ui.horizontal(|ui| {
            for (label, which) in [
                ("Portrait", Orientation::Portrait),
                ("Landscape", Orientation::Landscape),
            ] {
                if ui
                    .selectable_label(settings.orientation == which, label)
                    .clicked()
                {
                    settings.orientation = which;
                }
            }
        });
    });

    ui.add_space(Theme::SPACE_2);
    let mut pages = settings.pages as f64;
    pair(
        ui,
        ("Pages", |ui: &mut Ui| {
            ui.add(
                egui::DragValue::new(&mut pages)
                    .range(1.0..=2000.0)
                    .fixed_decimals(0),
            );
        }),
        ("Facing", |ui: &mut Ui| {
            ui.checkbox(&mut settings.facing_pages, "");
        }),
    );
    settings.pages = (pages.round() as u32).clamp(1, 2000);

    pair(
        ui,
        ("Margin", |ui: &mut Ui| {
            measure_bare(ui, &mut settings.margin, unit)
        }),
        ("Bleed", |ui: &mut Ui| {
            measure_bare(ui, &mut settings.bleed, unit)
        }),
    );

    ui.add_space(Theme::SPACE_3);
    ui.colored_label(Theme::text_muted(), "Colour");
    for intent in [Intent::Print, Intent::Screen] {
        if ui
            .selectable_label(settings.intent == intent, intent.label())
            .clicked()
        {
            settings.intent = intent;
        }
    }
    note(ui, settings);

    if settings.intent == Intent::Print {
        let mut ppi = settings.minimum_ppi;
        if field(ui, "Warn below", |ui| {
            ui.add(
                egui::DragValue::new(&mut ppi)
                    .range(36.0..=1200.0)
                    .suffix(" ppi")
                    .fixed_decimals(0),
            )
            .changed()
        }) {
            settings.minimum_ppi = ppi;
        }
    }
}

/// What the colour choice actually did, said in one line.
fn note(ui: &mut Ui, settings: &NewDocument) {
    let text = match settings.intent {
        Intent::Screen => "No press. Colour stays as it is typed, and nothing is proofed or \
             separated."
            .to_string(),
        Intent::Print => match press_name(settings) {
            Some(name) => format!(
                "Proofed and separated through {name}. Change it in Layout \u{203a} \
                 Document setup once you know the printer."
            ),
            // Said rather than hidden: a print document with no profile behind
            // it cannot soft-proof or export PDF/X, and finding that out at
            // export time is finding out too late.
            None => "No press profile is bundled with this build, so CMYK \
                     export and soft proofing are unavailable."
                .to_string(),
        },
    };
    ui.colored_label(Theme::text_muted(), text);
}

/// The bundled profile this intent will use, by its readable name.
fn press_name(settings: &NewDocument) -> Option<String> {
    let file = settings.intent.profile()?;
    tessera_color::profiles::bundled()
        .into_iter()
        .find(|b| b.path.file_name().is_some_and(|n| n == file))
        .map(|b| b.name)
}

/// Whether the canvas should show nothing at all.
///
/// The dialog is open and its preview is off, so the placeholder document
/// behind it is not a document anybody asked for. Drawing it would be showing a
/// page whose size somebody is in the middle of choosing.
pub fn showing_nothing(state: &TesseraApp) -> bool {
    state.new_document.open && !state.new_document.preview
}

/// Make the placeholder document match what the dialog is asking for.
///
/// **Written straight into the document, not through a command.** This is a
/// preview of something that does not exist yet: it must not enter undo, must
/// not make the document dirty, and must leave it recognisable as the untouched
/// blank that `add_document` replaces when Create is finally pressed. A preview
/// that dirtied the document would make Tessera ask whether to save a page
/// somebody only looked at.
pub fn sync_preview(state: &mut TesseraApp) {
    if !state.new_document.open || !state.new_document.preview {
        return;
    }
    // Only ever the placeholder. Somebody who opens File > New with work on
    // screen must not watch that work resize under them.
    if !state.active().current_path.is_none() || state.active().dirty {
        return;
    }
    if !state.active().document().frames.is_empty() {
        return;
    }

    let settings = state.new_document.clone();
    let (width, height) = settings.page();
    // undo-bracketed: nothing to bracket. This previews a document that does
    // not exist yet — the placeholder is replaced wholesale when Create is
    // pressed, and thrown away if it is not. Going through a command would put
    // "resize a page nobody has made" into the undo history of the document
    // that ends up being made, and would mark it dirty, so Tessera would ask
    // whether to save a page somebody only looked at.
    let document = state.active_mut().document_mut();

    let already = document.setup.facing_pages == settings.facing_pages
        && document.setup.margins == tessera_document::nodes::Margins::uniform(settings.margin)
        && document.setup.bleed == tessera_document::nodes::Insets::uniform(settings.bleed);
    let sized = document.pages.values().next().is_some_and(|p| {
        (p.bounds.width - width).abs() < 0.01 && (p.bounds.height - height).abs() < 0.01
    });
    let counted = document.page_ids().count() == settings.pages as usize;
    if already && sized && counted {
        // Nothing changed. Rebuilding every frame would reflow the spreads
        // sixty times a second for a page nobody is touching.
        return;
    }

    document.setup.facing_pages = settings.facing_pages;
    document.setup.margins = tessera_document::nodes::Margins::uniform(settings.margin);
    document.setup.bleed = tessera_document::nodes::Insets::uniform(settings.bleed);
    document.set_page_size(width, height);

    while document.page_ids().count() > settings.pages as usize {
        let Some(last) = document.page_ids().last() else {
            break;
        };
        document.remove_page(last);
    }
    while document.page_ids().count() < settings.pages as usize {
        document.add_page();
    }
    document.reflow_spreads();

    // The camera, so a page that has just changed size is still on screen.
    state.active_mut().fitted = false;
}

/// Build the document the dialog describes, and open it.
fn create(state: &mut TesseraApp) {
    let settings = state.new_document.clone();
    let (width, height) = settings.page();

    let mut document = crate::file_ops::starting_document();
    document.set_page_size(width, height);

    let setup = tessera_document::nodes::DocumentSetup {
        margins: tessera_document::nodes::Margins::uniform(settings.margin),
        bleed: tessera_document::nodes::Insets::uniform(settings.bleed),
        facing_pages: settings.facing_pages,
        ..document.setup
    };
    document.setup = setup;
    document.reflow_spreads();

    // Pages after the setup, so each one is made against the margins and the
    // bleed the document actually has rather than against the defaults.
    for _ in 1..settings.pages {
        document.add_page();
    }

    if let Some(file) = settings.intent.profile()
        && let Some(found) = tessera_color::profiles::bundled()
            .into_iter()
            .find(|b| b.path.file_name().is_some_and(|n| n == file))
        && let Ok(bytes) = std::fs::read(&found.path)
    {
        document.output_intent = Some(tessera_document::intent::OutputIntent {
            description: found.name.clone(),
            profile: bytes,
            rendering: tessera_document::intent::Rendering::default(),
        });
    }

    state.add_document(document, None);
    state.prefs.minimum_ppi = settings.minimum_ppi;
    state.status = Some(crate::app::Status::info("New document"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_would_be_accepted_by_a_printer() {
        // The whole test of a default: somebody who presses Return without
        // reading gets a document that is not wrong.
        let fresh = NewDocument::default();
        assert_eq!(fresh.preset, Some(PagePreset::A4));
        assert!(
            fresh.bleed > 0.0,
            "no bleed is a job that cannot be trimmed"
        );
        assert!(fresh.margin > 0.0);
        assert_eq!(fresh.intent, Intent::Print);
        assert!(fresh.facing_pages);
    }

    #[test]
    fn orientation_turns_the_page_rather_than_the_preset() {
        // The preset stays what it is. Choosing landscape A4 and reopening the
        // dialog must still say A4, not Custom.
        let mut settings = NewDocument::default();
        let (portrait_w, portrait_h) = settings.page();
        settings.orientation = Orientation::Landscape;
        let (landscape_w, landscape_h) = settings.page();

        assert_eq!(settings.preset, Some(PagePreset::A4));
        assert!((portrait_w - landscape_h).abs() < 0.01);
        assert!((portrait_h - landscape_w).abs() < 0.01);
    }

    #[test]
    fn typing_a_size_that_is_a_paper_still_names_it() {
        // `matching` answering honestly, rather than a special case. Somebody
        // who types A4's measurements has chosen A4.
        let (w, h) = PagePreset::A4.size();
        assert_eq!(PagePreset::matching(w, h), Some(PagePreset::A4));
    }

    #[test]
    fn screen_documents_name_no_press() {
        // And print ones do. The whole of what the colour choice decides.
        assert!(Intent::Screen.profile().is_none());
        assert!(Intent::Print.profile().is_some());
    }

    #[test]
    fn the_dialog_makes_the_pages_it_was_asked_for() {
        let mut state = TesseraApp::headless();
        state.new_document = NewDocument {
            pages: 5,
            ..Default::default()
        };
        create(&mut state);
        assert_eq!(state.active().document().page_ids().count(), 5);
    }

    #[test]
    fn a_print_document_carries_its_press() {
        // Without an output intent there is no soft proof, no CMYK export and
        // no PDF/X. Skipped where no profile is bundled, which is a checkout
        // that has not run `tools/vendor-profiles.py`.
        if tessera_color::profiles::bundled().is_empty() {
            return;
        }
        let mut state = TesseraApp::headless();
        state.new_document = NewDocument {
            intent: Intent::Print,
            ..Default::default()
        };
        create(&mut state);
        assert!(
            state.active().document().output_intent.is_some(),
            "a print document was made with no press"
        );
    }

    #[test]
    fn a_screen_document_carries_none() {
        let mut state = TesseraApp::headless();
        state.new_document = NewDocument {
            intent: Intent::Screen,
            ..Default::default()
        };
        create(&mut state);
        assert!(state.active().document().output_intent.is_none());
    }
}
