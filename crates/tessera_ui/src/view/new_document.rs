//! The New Document dialog.
//!
//! Tessera used to open with a document already made. That is convenient for
//! about ten seconds and wrong for everything after: a page size, a bleed and a
//! press are decisions a job is built on, and changing them later means moving
//! everything that was laid out against the old ones. Every layout tool asks
//! first for that reason.
//!
//! ## Laid out as InDesign's
//!
//! Tabs for what the job is for, a grid of sizes drawn as the pages they are,
//! and the chosen one's details beside them. A page size is recognised on
//! sight long before "148 × 210" is read, and the details column is where the
//! numbers are for the job no preset fits.
//!
//! ## The colour question is Print or Screen, not which profile
//!
//! "Which ICC profile" is a question with about nine answers, seven of which
//! differ in ways only a printer can explain. Somebody starting a job knows
//! whether it is going on screen or on a press; they do not yet know whether
//! their printer runs CRPC3 or CRPC6, and asking them to guess makes the answer
//! worse than a sensible default. The tab answers the question they can; the
//! press can be changed in document setup once there is a printer to ask.
//!
//! ## Everything has a default, and the defaults are somebody's job
//!
//! A4, facing pages, sensible margins and a 3mm bleed. Somebody who presses
//! Return without reading gets a document a printer would accept, which is the
//! whole test of a default.

use egui::{Rect, Sense, Ui, Vec2};

use crate::app::TesseraApp;
use crate::icons::Icon;
use crate::theme::Theme;
use tessera_document::nodes::{Insets, Margins, Orientation, PagePreset};
use tessera_geometry::Unit;

/// The bleed a commercial printer expects, in millimetres.
///
/// Three is the European convention and an eighth of an inch (3.175mm) the
/// American one; they are close enough that three works for both, and a
/// document with *some* bleed is immeasurably better than one with none.
const USUAL_BLEED_MM: f64 = 3.0;

/// The margin a page starts with, in millimetres.
const USUAL_MARGIN_MM: f64 = 12.7;

/// The space between column guides a page starts with: a pica, InDesign's.
const USUAL_GUTTER_PT: f64 = 12.0;

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
    fn tab(self) -> &'static str {
        match self {
            Intent::Print => "Print",
            Intent::Screen => "Screen",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Intent::Print => "CMYK, for a press: paper sizes, bleed, and a press to proof against",
            Intent::Screen => "RGB, for screens: sizes in pixels, no bleed and no press",
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

/// The sizes the Screen tab offers, in pixels.
///
/// A pixel is a point here, as it is in InDesign's web and mobile intents: a
/// 1920-pixel slide is 1920 points wide and exports at that many pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenPreset {
    FullHd,
    Hd,
    WebPage,
    Slide,
    Square,
    Post,
    Story,
}

impl ScreenPreset {
    /// The commonest first, as the paper sizes are.
    pub const ALL: [ScreenPreset; 7] = [
        ScreenPreset::FullHd,
        ScreenPreset::Hd,
        ScreenPreset::WebPage,
        ScreenPreset::Slide,
        ScreenPreset::Square,
        ScreenPreset::Post,
        ScreenPreset::Story,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ScreenPreset::FullHd => "Full HD",
            ScreenPreset::Hd => "HD",
            ScreenPreset::WebPage => "Web page",
            ScreenPreset::Slide => "Slide 4:3",
            ScreenPreset::Square => "Square post",
            ScreenPreset::Post => "Portrait post",
            ScreenPreset::Story => "Story",
        }
    }

    /// `(width, height)` in pixels, which are points.
    pub fn size(self) -> (f64, f64) {
        match self {
            ScreenPreset::FullHd => (1920.0, 1080.0),
            ScreenPreset::Hd => (1280.0, 720.0),
            ScreenPreset::WebPage => (1280.0, 800.0),
            ScreenPreset::Slide => (1024.0, 768.0),
            ScreenPreset::Square => (1080.0, 1080.0),
            ScreenPreset::Post => (1080.0, 1350.0),
            ScreenPreset::Story => (1080.0, 1920.0),
        }
    }

    /// The preset a page of these measurements is, the right way round only.
    ///
    /// Unlike paper, a screen size is its orientation: Full HD turned on its
    /// side is a story, and matching either way round named them both.
    pub fn matching(width: f64, height: f64) -> Option<Self> {
        Self::ALL.into_iter().find(|p| {
            let (w, h) = p.size();
            (w - width).abs() < 0.5 && (h - height).abs() < 0.5
        })
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
    pub margins: Margins,
    /// Column guides across the type area; one is no division.
    pub columns: u8,
    pub gutter: f64,
    pub bleed: f64,
    /// Room past the bleed for notes to the printer, trimmed off with it.
    pub slug: f64,
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
            margins: Margins::uniform(USUAL_MARGIN_MM * MM),
            columns: 1,
            gutter: USUAL_GUTTER_PT,
            bleed: USUAL_BLEED_MM * MM,
            slug: 0.0,
            intent: Intent::Print,
            minimum_ppi: 300.0,
            preview: true,
        }
    }
}

impl NewDocument {
    pub fn validation_error(&self) -> Option<&'static str> {
        if !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.0
            || self.height <= 0.0
        {
            return Some("Enter a page width and height greater than zero.");
        }
        if !(1..=2000).contains(&self.pages) {
            return Some("Choose between 1 and 2,000 pages.");
        }
        let m = self.margins;
        if [m.top, m.bottom, m.inside, m.outside]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Some("Enter margins of zero or greater.");
        }
        let (width, height) = self.page();
        if m.top + m.bottom >= height || m.inside + m.outside >= width {
            return Some("Reduce the margins so there is space for content on the page.");
        }
        if !(1..=24).contains(&self.columns) {
            return Some("Choose between 1 and 24 columns.");
        }
        if !self.gutter.is_finite() || self.gutter < 0.0 {
            return Some("Enter a gutter of zero or greater.");
        }
        if !self.bleed.is_finite() || self.bleed < 0.0 {
            return Some("Enter a bleed of zero or greater.");
        }
        if !self.slug.is_finite() || self.slug < 0.0 {
            return Some("Enter a slug of zero or greater.");
        }
        if !self.minimum_ppi.is_finite() || !(36.0..=1200.0).contains(&self.minimum_ppi) {
            return Some("Choose a resolution warning between 36 and 1,200 ppi.");
        }
        None
    }

    /// The page size as it will actually be made, orientation applied.
    pub fn page(&self) -> (f64, f64) {
        self.orientation.apply(self.width, self.height)
    }

    /// Take a paper size, keeping the orientation already chosen.
    fn take(&mut self, preset: PagePreset) {
        let (w, h) = preset.size();
        self.preset = Some(preset);
        self.width = w;
        self.height = h;
    }

    /// Take a screen size, which comes with its orientation: a story is tall
    /// and a slide is wide, and turning one is making something else.
    fn take_screen(&mut self, preset: ScreenPreset) {
        let (w, h) = preset.size();
        self.preset = None;
        self.width = w;
        self.height = h;
        self.orientation = Orientation::of(w, h);
    }

    /// Change what the job is for, and with it the defaults that follow from
    /// that: a press wants paper, facing pages and bleed; a screen wants
    /// pixels and has no fold and nothing to trim.
    pub fn switch(&mut self, intent: Intent) {
        if self.intent == intent {
            return;
        }
        self.intent = intent;
        match intent {
            Intent::Print => {
                self.take(PagePreset::A4);
                self.orientation = Orientation::Portrait;
                self.facing_pages = true;
                self.bleed = USUAL_BLEED_MM * MM;
            }
            Intent::Screen => {
                self.take_screen(ScreenPreset::FullHd);
                self.facing_pages = false;
                self.bleed = 0.0;
                self.slug = 0.0;
            }
        }
    }

    /// The name of the size as it stands, or "Custom" when it is none.
    fn size_name(&self) -> &'static str {
        match self.intent {
            Intent::Print => self.preset.map_or("Custom", PagePreset::name),
            Intent::Screen => {
                let (width, height) = self.page();
                ScreenPreset::matching(width, height).map_or("Custom", ScreenPreset::name)
            }
        }
    }
}

/// How wide the details column is, beside the presets.
const DETAILS: f32 = 264.0;

/// One preset's card.
const CARD: Vec2 = Vec2::new(94.0, 100.0);

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

    let screen = ctx.content_rect();
    let response = egui::Modal::new(egui::Id::new("new-document"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            let width = (screen.width() - 80.0).clamp(320.0, 780.0);
            ui.set_width(width);
            ui.heading("New document");
            ui.add_space(Theme::space_2());
            intent_tabs(ui, &mut settings);
            ui.add_space(Theme::space_3());

            // Two columns while both have room, one above the other when not.
            let tall = (screen.height() - 240.0).clamp(200.0, 500.0);
            let gap = Theme::space_5();
            if width >= 2.0 * DETAILS + gap {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(width - DETAILS - gap);
                        presets(ui, &mut settings, unit, tall);
                    });
                    ui.add_space(gap);
                    ui.vertical(|ui| {
                        ui.set_width(DETAILS);
                        egui::ScrollArea::vertical()
                            .id_salt("new-document-details")
                            .auto_shrink([false, false])
                            .max_height(tall)
                            .show(ui, |ui| details(ui, &mut settings, unit));
                    });
                });
            } else {
                presets(ui, &mut settings, unit, tall / 2.0);
                ui.add_space(Theme::space_3());
                egui::ScrollArea::vertical()
                    .id_salt("new-document-details")
                    .auto_shrink([false, false])
                    .max_height(tall / 2.0)
                    .show(ui, |ui| details(ui, &mut settings, unit));
            }

            ui.add_space(Theme::space_3());
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut settings.preview, "Preview");
                match settings.validation_error() {
                    Some(error) => {
                        ui.colored_label(Theme::error(), error);
                    }
                    None => {
                        ui.weak(summary(&settings, unit));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    make = ui
                        .add_enabled(
                            settings.validation_error().is_none(),
                            super::primary_button("Create"),
                        )
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        });

    state.new_document = settings;
    if make {
        create(state);
        state.new_document.open = false;
    } else if cancel || response.should_close() {
        // There is always an active document, including the startup blank.
        state.new_document.open = false;
    }
}

/// "1 page · 210 × 297 mm · Facing pages", for the footer.
fn summary(settings: &NewDocument, unit: Unit) -> String {
    let (width, height) = settings.page();
    let unit = shown_unit(settings, unit);
    format!(
        "{} page{} · {} × {} {} · {}",
        settings.pages,
        if settings.pages == 1 { "" } else { "s" },
        number(unit.from_points(width)),
        number(unit.from_points(height)),
        unit.suffix().trim(),
        if settings.facing_pages {
            "Facing pages"
        } else {
            "Single pages"
        }
    )
}

/// The unit sizes are spoken in: pixels for a screen, whatever the rulers
/// count in for print.
fn shown_unit(settings: &NewDocument, unit: Unit) -> Unit {
    match settings.intent {
        Intent::Screen => Unit::Pixels,
        Intent::Print => unit,
    }
}

/// A measurement without the zeros nobody reads: "210", "8.5", "184.2".
///
/// One decimal: a card is a name for a size, and "184.15 × 266.7 mm" ran off
/// the edge of it. The fields hold the exact figure.
fn number(value: f64) -> String {
    let text = format!("{value:.1}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Print or Screen, as InDesign's intent tabs: the text of the one chosen,
/// underlined in the accent, and the other quieter.
fn intent_tabs(ui: &mut Ui, settings: &mut NewDocument) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = Theme::space_1();
        for intent in [Intent::Print, Intent::Screen] {
            let on = settings.intent == intent;
            let font = egui::FontId::proportional(Theme::TYPE_LG);
            let galley =
                ui.painter()
                    .layout_no_wrap(intent.tab().to_string(), font, Theme::text_primary());
            let size = galley.size() + Vec2::new(2.0 * Theme::space_3(), Theme::space_2() + 2.0);
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            let colour = if on || response.hovered() {
                Theme::text_primary()
            } else {
                Theme::text_muted()
            };
            let painter = ui.painter_at(rect);
            painter.galley_with_override_text_color(
                egui::pos2(rect.left() + Theme::space_3(), rect.top()),
                galley,
                colour,
            );
            if on {
                painter.rect_filled(
                    Rect::from_min_max(
                        egui::pos2(rect.left() + Theme::space_3(), rect.bottom() - 2.0),
                        egui::pos2(rect.right() - Theme::space_3(), rect.bottom()),
                    ),
                    1.0,
                    Theme::accent(),
                );
            }
            if response.has_focus() {
                painter.rect_stroke(
                    rect,
                    Theme::RADIUS,
                    egui::Stroke::new(1.0, Theme::focus()),
                    egui::StrokeKind::Inside,
                );
            }
            let response = crate::icons::reads_as(
                response,
                intent.tab(),
                egui::WidgetType::RadioButton,
                Some(on),
            )
            .on_hover_text(intent.hint());
            if response.clicked() {
                settings.switch(intent);
            }
        }
    });
    // The rule the tabs stand on, so they read as tabs over a page and not as
    // two words floating above it.
    let rule = ui.min_rect().bottom();
    ui.painter().hline(
        ui.min_rect().x_range(),
        rule,
        egui::Stroke::new(1.0, Theme::rule()),
    );
}

/// The tab's sizes, as cards to pick from.
fn presets(ui: &mut Ui, settings: &mut NewDocument, unit: Unit, height: f32) {
    ui.label(
        egui::RichText::new("Blank document presets")
            .size(Theme::TYPE_SM)
            .color(Theme::text_muted()),
    );
    ui.add_space(Theme::space_1());
    egui::ScrollArea::vertical()
        .id_salt("new-document-presets")
        .max_height(height)
        // Held at its height whatever the tab holds, so switching from sixteen
        // papers to seven screens does not make the dialog jump.
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(Theme::space_2());
                match settings.intent {
                    Intent::Print => {
                        for preset in PagePreset::ALL {
                            // Turned as the page will be, so choosing landscape
                            // shows landscape pages rather than only saying so.
                            let (w, h) =
                                settings.orientation.apply(preset.size().0, preset.size().1);
                            let on = settings.preset == Some(preset);
                            if card(ui, preset.name(), (w, h), unit, on).clicked() {
                                settings.take(preset);
                            }
                        }
                    }
                    Intent::Screen => {
                        let (width, height) = settings.page();
                        let current = ScreenPreset::matching(width, height);
                        for preset in ScreenPreset::ALL {
                            let on = current == Some(preset);
                            let size = preset.size();
                            if card(ui, preset.name(), size, Unit::Pixels, on).clicked() {
                                settings.take_screen(preset);
                            }
                        }
                    }
                }
            });
        });
}

/// One preset: the page drawn at its own proportions, its name, its size.
fn card(ui: &mut Ui, name: &str, (w, h): (f64, f64), unit: Unit, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
    crate::view::panels::paint_toggle_frame(ui, rect, &response, on);
    let painter = ui.painter_at(rect);

    // The page, fitted into the top of the card and centred there.
    let room = Vec2::new(rect.width() - 2.0 * Theme::space_4(), 50.0);
    let scale = (f64::from(room.x) / w).min(f64::from(room.y) / h) as f32;
    let page = Rect::from_center_size(
        egui::pos2(
            rect.center().x,
            rect.top() + Theme::space_2() + room.y / 2.0,
        ),
        Vec2::new(w as f32 * scale, h as f32 * scale),
    );
    painter.rect_filled(page, 1.0, egui::Color32::WHITE);
    painter.rect_stroke(
        page,
        1.0,
        egui::Stroke::new(1.0, if on { Theme::accent() } else { Theme::border() }),
        egui::StrokeKind::Outside,
    );

    let size = format!(
        "{} × {} {}",
        number(unit.from_points(w)),
        number(unit.from_points(h)),
        unit.suffix().trim()
    );
    let text_top = rect.top() + Theme::space_2() + room.y + Theme::space_2();
    painter.text(
        egui::pos2(rect.center().x, text_top),
        egui::Align2::CENTER_TOP,
        name,
        egui::FontId::proportional(Theme::TYPE_MD),
        Theme::text_primary(),
    );
    painter.text(
        egui::pos2(rect.center().x, text_top + Theme::TYPE_MD + 4.0),
        egui::Align2::CENTER_TOP,
        &size,
        egui::FontId::proportional(Theme::TYPE_SM),
        Theme::text_muted(),
    );
    crate::icons::reads_as(
        response,
        format!("{name}, {size}"),
        egui::WidgetType::RadioButton,
        Some(on),
    )
}

/// The chosen size's details: everything a preset does not decide.
fn details(ui: &mut Ui, settings: &mut NewDocument, unit: Unit) {
    use crate::view::panels::{icon_choices, linked_edges, measure_bare, pair};

    ui.spacing_mut().item_spacing.y = Theme::space_2();
    ui.label(egui::RichText::new(settings.size_name()).strong());

    let unit = shown_unit(settings, unit);
    let (mut width, mut height) = settings.page();
    let (a, b) = pair(
        ui,
        ("Width", |ui: &mut Ui| measure_bare(ui, &mut width, unit)),
        ("Height", |ui: &mut Ui| measure_bare(ui, &mut height, unit)),
    );
    if a || b {
        settings.width = width;
        settings.height = height;
        settings.orientation = Orientation::of(width, height);
        // Typing a size makes it Custom, unless it happens to be a paper. That
        // is not a special case: it is `matching` answering honestly.
        settings.preset = PagePreset::matching(settings.width, settings.height);
    }

    icon_choices(
        ui,
        "Orientation",
        &mut settings.orientation,
        &[
            (
                Icon::PagePortrait,
                "Portrait",
                "Taller than it is wide",
                Orientation::Portrait,
            ),
            (
                Icon::PageLandscape,
                "Landscape",
                "Wider than it is tall",
                Orientation::Landscape,
            ),
        ],
    );

    let mut pages = f64::from(settings.pages);
    pair(
        ui,
        ("Pages", |ui: &mut Ui| {
            ui.add(
                egui::DragValue::new(&mut pages)
                    .range(1.0..=2000.0)
                    .fixed_decimals(0),
            );
        }),
        // No word above it: the box carries its own, and a label saying
        // "Facing" over a box saying "Facing pages" would say it twice.
        ("", |ui: &mut Ui| {
            ui.checkbox(&mut settings.facing_pages, "Facing pages")
                .on_hover_text("Left and right pages, bound at the fold, as a book is");
        }),
    );
    settings.pages = (pages.round() as u32).clamp(1, 2000);

    let mut columns = f64::from(settings.columns);
    pair(
        ui,
        ("Columns", |ui: &mut Ui| {
            ui.add(
                egui::DragValue::new(&mut columns)
                    .range(1.0..=24.0)
                    .fixed_decimals(0),
            );
        }),
        ("Gutter", |ui: &mut Ui| {
            measure_bare(ui, &mut settings.gutter, unit)
        }),
    );
    settings.columns = (columns.round() as u8).clamp(1, 24);

    let labels = if settings.facing_pages {
        ["Top", "Bottom", "Inside", "Outside"]
    } else {
        ["Top", "Bottom", "Left", "Right"]
    };
    let m = &mut settings.margins;
    linked_edges(
        ui,
        egui::Id::new("new-document-margins"),
        "Margins",
        labels,
        [&mut m.top, &mut m.bottom, &mut m.inside, &mut m.outside],
        unit,
    );

    if settings.intent == Intent::Print {
        ui.add_space(Theme::space_1());
        pair(
            ui,
            ("Bleed", |ui: &mut Ui| {
                measure_bare(ui, &mut settings.bleed, unit)
            }),
            ("Slug", |ui: &mut Ui| {
                measure_bare(ui, &mut settings.slug, unit)
            }),
        );
    }

    ui.add_space(Theme::space_2());
    ui.label(egui::RichText::new("Colour").strong());
    note(ui, settings);
    if settings.intent == Intent::Print {
        // On one line: a sentence with a number in it, not a field with a
        // label two lines tall.
        ui.horizontal(|ui| {
            let words = "Warn when images are below";
            ui.label(
                egui::RichText::new(words)
                    .size(Theme::TYPE_SM)
                    .color(Theme::text_muted()),
            );
            crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut settings.minimum_ppi)
                        .range(36.0..=1200.0)
                        .suffix(" ppi")
                        .fixed_decimals(0),
                ),
                words,
            );
        });
    }
}

/// What the tab actually did to colour, said in a line or two.
fn note(ui: &mut Ui, settings: &NewDocument) {
    let text = match settings.intent {
        Intent::Screen => "RGB. No press: colour stays as it is typed, and nothing is \
             proofed or separated."
            .to_string(),
        Intent::Print => match press_name(settings) {
            Some(name) => format!(
                "CMYK, proofed and separated through {name}. Change the press in \
                 Layout \u{203a} Document setup once you know the printer."
            ),
            // Said rather than hidden: a print document with no profile behind
            // it cannot soft-proof or export PDF/X, and finding that out at
            // export time is finding out too late.
            None => "No press profile is bundled with this build, so CMYK \
                     export and soft proofing are unavailable."
                .to_string(),
        },
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(Theme::TYPE_SM)
                .color(Theme::text_muted()),
        )
        .wrap(),
    );
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
    state.new_document.open
        && !state.new_document.preview
        && state.active().current_path.is_none()
        && !state.active().dirty
        && state.active().document().frames.is_empty()
}

/// The setup a document made from `settings` has, on top of `base`.
fn setup_of(
    settings: &NewDocument,
    base: tessera_document::nodes::DocumentSetup,
) -> tessera_document::nodes::DocumentSetup {
    tessera_document::nodes::DocumentSetup {
        margins: settings.margins,
        bleed: Insets::uniform(settings.bleed),
        slug: Insets::uniform(settings.slug),
        facing_pages: settings.facing_pages,
        columns: settings.columns,
        column_gutter: settings.gutter,
        ..base
    }
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
    if !state.new_document.open
        || !state.new_document.preview
        || state.new_document.validation_error().is_some()
    {
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

    let wanted = setup_of(&settings, document.setup);
    let already = document.setup == wanted;
    let sized = document.pages.values().next().is_some_and(|p| {
        (p.bounds.width - width).abs() < 0.01 && (p.bounds.height - height).abs() < 0.01
    });
    let counted = document.page_ids().count() == settings.pages as usize;
    if already && sized && counted {
        // Nothing changed. Rebuilding every frame would reflow the spreads
        // sixty times a second for a page nobody is touching.
        return;
    }

    document.setup = wanted;
    document.set_page_size(width, height);

    while document.page_ids().count() > settings.pages as usize {
        let Some(last) = document.page_ids().last() else {
            break;
        };
        if !document.remove_page(last) {
            break;
        }
    }
    while document.page_ids().count() < settings.pages as usize {
        document.add_page();
    }
    document.reflow_spreads();

    // The camera, so a page that has just changed size is still on screen.
    state.active_mut().fitted = false;
}

/// Make the document `state.new_document` describes and open it. Public so
/// the bridge can make one from a model's choices without the dialog.
pub fn create(state: &mut TesseraApp) {
    if let Some(error) = state.new_document.validation_error() {
        state.status = Some(crate::app::Status::error(error));
        return;
    }
    let settings = state.new_document.clone();
    let (width, height) = settings.page();

    let mut document = crate::file_ops::starting_document();
    document.set_page_size(width, height);
    document.setup = setup_of(&settings, document.setup);
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
    // Page setup is authored work even before the first frame is drawn.
    // Protect it on close and do not reuse it as the startup placeholder.
    state.active_mut().dirty = true;
    state.prefs.minimum_ppi = settings.minimum_ppi;
    state.status = Some(crate::app::Status::info("New document"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_setup_never_reaches_preview_or_creation() {
        let invalid = [
            NewDocument {
                pages: 0,
                ..Default::default()
            },
            NewDocument {
                pages: 2001,
                ..Default::default()
            },
            NewDocument {
                width: 0.0,
                ..Default::default()
            },
            NewDocument {
                height: f64::NAN,
                ..Default::default()
            },
            NewDocument {
                margins: Margins::uniform(1000.0),
                ..Default::default()
            },
            NewDocument {
                margins: Margins {
                    top: -1.0,
                    ..Margins::uniform(10.0)
                },
                ..Default::default()
            },
            NewDocument {
                columns: 0,
                ..Default::default()
            },
            NewDocument {
                bleed: -1.0,
                ..Default::default()
            },
            NewDocument {
                slug: f64::INFINITY,
                ..Default::default()
            },
        ];
        for mut settings in invalid {
            let mut state = TesseraApp::headless();
            let before = state.active().document().clone();
            settings.open = true;
            state.new_document = settings;
            sync_preview(&mut state);
            create(&mut state);
            assert_eq!(
                serde_json::to_value(state.active().document()).unwrap(),
                serde_json::to_value(&before).unwrap()
            );
            assert!(state.status.as_ref().unwrap().is_error);
        }
    }

    #[test]
    fn a_configured_empty_document_is_work_not_a_placeholder() {
        let mut state = TesseraApp::headless();
        state.new_document.pages = 5;
        create(&mut state);
        let first = state.active;
        assert!(state.active().dirty);
        state.new_document.pages = 2;
        state.new_document.open = true;
        sync_preview(&mut state);
        assert_eq!(state.active().document().page_ids().count(), 5);
        create(&mut state);
        assert_ne!(state.active, first);
        assert_eq!(state.documents[first].document().page_ids().count(), 5);
    }

    #[test]
    fn escape_cancels_new_with_one_document_without_changing_its_work() {
        let mut state = TesseraApp::headless();
        let bounds = state.first_page_bounds();
        crate::apply(&mut state, crate::Command::AddRectangle(bounds));
        state.new_document.open = true;
        state.new_document.preview = false;
        assert!(!showing_nothing(&state));
        let ctx = egui::Context::default();
        let _ =
            crate::headless_frame::frame(&ctx, Default::default(), |ui| show(ui.ctx(), &mut state));
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = crate::headless_frame::frame(&ctx, input, |ui| show(ui.ctx(), &mut state));
        assert!(!state.new_document.open);
        assert_eq!(state.documents.len(), 1);
        assert_eq!(state.active().document().frames.len(), 1);
        assert!(state.active().dirty);
    }

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
        assert!(fresh.margins.top > 0.0 && fresh.margins.inside > 0.0);
        assert_eq!(fresh.columns, 1);
        assert_eq!(fresh.intent, Intent::Print);
        assert!(fresh.facing_pages);
        assert!(fresh.validation_error().is_none());
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
        assert_eq!(
            ScreenPreset::matching(1080.0, 1920.0),
            Some(ScreenPreset::Story)
        );
        assert_eq!(
            ScreenPreset::matching(1920.0, 1080.0),
            Some(ScreenPreset::FullHd)
        );
    }

    #[test]
    fn the_screen_tab_brings_a_screens_defaults_and_print_brings_them_back() {
        let mut settings = NewDocument::default();
        settings.switch(Intent::Screen);
        assert_eq!(settings.intent, Intent::Screen);
        assert_eq!(settings.page(), ScreenPreset::FullHd.size());
        assert_eq!(settings.size_name(), "Full HD");
        assert!(!settings.facing_pages, "a screen has no fold");
        assert_eq!(settings.bleed, 0.0, "and nothing to trim");

        settings.switch(Intent::Print);
        assert_eq!(settings.preset, Some(PagePreset::A4));
        assert_eq!(settings.orientation, Orientation::Portrait);
        assert!(settings.facing_pages);
        assert!(settings.bleed > 0.0);
        assert!(settings.validation_error().is_none());
    }

    #[test]
    fn a_tall_screen_preset_stays_tall() {
        // A story is portrait. Taken with the orientation a slide left behind,
        // `page()` would turn it on its side.
        let mut settings = NewDocument::default();
        settings.switch(Intent::Screen);
        settings.take_screen(ScreenPreset::Story);
        assert_eq!(settings.page(), (1080.0, 1920.0));
    }

    #[test]
    fn the_details_reach_the_document() {
        let mut state = TesseraApp::headless();
        state.new_document = NewDocument {
            margins: Margins {
                top: 20.0,
                bottom: 30.0,
                inside: 40.0,
                outside: 50.0,
            },
            columns: 3,
            gutter: 14.0,
            slug: 9.0,
            ..Default::default()
        };
        create(&mut state);
        let setup = &state.active().document().setup;
        assert_eq!(setup.margins.bottom, 30.0);
        assert_eq!(setup.margins.outside, 50.0);
        assert_eq!(setup.columns, 3);
        assert_eq!(setup.column_gutter, 14.0);
        assert_eq!(setup.slug, Insets::uniform(9.0));
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
