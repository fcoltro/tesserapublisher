//! The control bar: one row, describing whatever is selected.
//!
//! The surface a layout artist touches most, and the reason InDesign's
//! right-hand panels can stay shut most of the time. Tessera had none, so
//! everything had been pushed into the inspector instead — which is why that
//! column was too long to read and too narrow to fit.
//!
//! It is **one row in one place**, and what it describes is named at its left
//! end, so it is never ambiguous which thing the numbers belong to. Geometry
//! lives here and nowhere else: the Properties section keeps scale and shear,
//! which are asked for rarely and read badly in a row.

use egui::Ui;

use crate::app::TesseraApp;
use crate::theme::Theme;

/// The reference proxy, at the size this bar has room for.
///
/// Smaller than the one in a panel. The bar is a row, and the full proxy is
/// taller than a row.
pub const PROXY: f32 = 30.0;

/// The padding above and below the row's tallest thing.
const PADDING: f32 = 5.0;

/// How tall the bar is. One row plus its padding, fixed — a bar that changed
/// height with its contents would move the canvas every time the selection
/// changed.
///
/// **Derived from the tallest thing in it**, not chosen. It was chosen, at 32,
/// and the proxy is 45: the bottom row of reference points was clipped away
/// entirely, and a clipped proxy reads as a proxy that will not change rather
/// than as one that is cut off. `the_bar_fits_its_proxy` is the guard.
pub const HEIGHT: f32 = PROXY + PADDING * 2.0;

// Checked by the compiler rather than by a test, because both sides are
// constants and a test can only fail after somebody has already built and run
// it. The bar was 32 tall holding a 45-point proxy: the bottom row of reference
// points was clipped away entirely, which reads as a proxy that will not change
// rather than as one that is cut off.
const _: () = assert!(HEIGHT >= PROXY);

// The bar is a row, and the panel proxy is taller than a row. Making the bar
// tall enough for the full one would put a band of chrome across the window.
const _: () = assert!(PROXY < crate::view::panels::PROXY);

/// How many sides the next polygon has, and how deep a star it is.
///
/// Written straight to the preferences, which is where they live: somebody
/// drawing hexagons is drawing hexagons all afternoon, and a tool that forgot
/// between drags would be one they fought.
fn polygon_options(ui: &mut Ui, state: &mut TesseraApp) {
    use tessera_document::polygon::{FEWEST_SIDES, MOST_SIDES};

    let mut sides = f64::from(state.prefs.polygon_sides);
    label(ui, "Sides");
    if ui
        .add(
            egui::DragValue::new(&mut sides)
                .range(f64::from(FEWEST_SIDES)..=f64::from(MOST_SIDES))
                .speed(0.15)
                .fixed_decimals(0),
        )
        .changed()
    {
        state.prefs.polygon_sides = sides.round() as u32;
        crate::prefs::remember(state);
    }

    // As a percentage, because "how far in" is a proportion and nobody thinks
    // about it in points.
    let mut inset = state.prefs.polygon_inset * 100.0;
    label(ui, "Star");
    if ui
        .add(
            egui::DragValue::new(&mut inset)
                .range(0.0..=95.0)
                .suffix("%")
                .speed(0.5)
                .fixed_decimals(0),
        )
        .changed()
    {
        state.prefs.polygon_inset = inset / 100.0;
        crate::prefs::remember(state);
    }
}

/// What the bar is describing right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Subject {
    /// Nothing selected: the document itself.
    Document,
    /// One object.
    Object,
    /// Several, which have no single geometry between them.
    Several(usize),
    /// A caret in text.
    Text,
}

/// What the bar should describe, given what is selected.
///
/// A caret wins over the frame holding it: while you are typing, the thing
/// being worked on is the text, not the box round it.
pub fn subject(state: &TesseraApp) -> Subject {
    if state.active().editing.is_some() {
        return Subject::Text;
    }
    match state.active().selection.len() {
        0 => Subject::Document,
        1 => Subject::Object,
        n => Subject::Several(n),
    }
}

impl Subject {
    pub fn name(self) -> &'static str {
        match self {
            Subject::Document => "Document",
            Subject::Object => "Object",
            Subject::Several(_) => "Objects",
            Subject::Text => "Text",
        }
    }
}

pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    let subject = subject(state);

    egui::ScrollArea::horizontal()
        .id_salt("control-bar-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = Theme::space_2();

                // What the row is about, at the left end, always.
                label(ui, subject.name());
                separator(ui);

                // A tool with options of its own says so here, before the selection
                // does. The polygon's sides decide what the *next* drag draws, so they
                // belong with the tool rather than with whatever happens to be
                // selected — which may be nothing at all.
                if state.active_tool == crate::tools::Tool::Polygon {
                    polygon_options(ui, state);
                    separator(ui);
                }

                match subject {
                    Subject::Object => object(ui, state),
                    Subject::Text => crate::view::panels::type_row(ui, state),
                    Subject::Several(n) => {
                        ui.colored_label(
                            Theme::text_muted(),
                            format!("{n} selected — no single geometry between them"),
                        );
                    }
                    Subject::Document => crate::view::panels::page_row(ui, state),
                }
            });
        });
}

fn object(ui: &mut Ui, state: &mut TesseraApp) {
    let Some(id) = state.active().selection.single() else {
        return;
    };
    let Some(frame) = state.active().document().frame(id).cloned() else {
        return;
    };
    crate::view::panels::transform_row(ui, state, id, &frame);
}

/// A muted caption naming what follows.
pub fn label(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(Theme::TYPE_SM)
                .color(Theme::text_muted()),
        )
        .selectable(false),
    );
}

/// A hairline between groups in the row.
pub fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(1.0, HEIGHT - Theme::space_3()),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, Theme::border());
}
