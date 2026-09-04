//! The tool strip, the inspector and the status bar.

use egui::{Sense, Ui, Vec2};
use tessera_color::Color;
use tessera_geometry::Unit;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;
use crate::tools::Tool;

// --- tool strip --------------------------------------------------------

pub fn tool_strip(ui: &mut Ui, state: &mut TesseraApp) {
    ui.vertical(|ui| {
        ui.add_space(Theme::SPACING_SM);
        for tool in Tool::ALL {
            if tool_button(ui, tool, state.active_tool == tool).clicked() {
                state.active_tool = tool;
            }
        }
    });
}

/// Icons come from Lucide, painted through `egui::Painter` from path data
/// rather than loaded as assets — so they stay crisp at any DPI and re-tint
/// with the theme. See [`crate::icons`].
fn tool_button(ui: &mut Ui, tool: Tool, active: bool) -> egui::Response {
    let size = Vec2::splat(Theme::TOOL_SIZE);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let bg = if active {
        Theme::ACCENT
    } else if response.hovered() {
        Theme::BORDER
    } else {
        Theme::PANEL_BG_ALT
    };
    let fg = if active {
        Theme::PANEL_BG
    } else {
        Theme::TEXT_PRIMARY
    };

    ui.painter().rect_filled(rect, Theme::RADIUS, bg);
    // Inset so the 24-unit icon grid does not touch the button edge.
    let inset = Theme::TOOL_SIZE * 0.22;
    crate::icons::paint(ui.painter(), rect.shrink(inset), tool.icon(), fg);

    response.on_hover_text(format!("{} ({:?})", tool.label(), tool.shortcut()))
}

// --- inspector ---------------------------------------------------------

pub fn inspector(ui: &mut Ui, state: &mut TesseraApp) {
    ui.heading("Properties");
    ui.separator();

    if state.active().selection.is_empty() {
        document_setup(ui, state);
        return;
    }

    // Geometry fields edit one frame. With several selected there is no single
    // value to show, and silently editing only the first would be worse than
    // saying so.
    let Some(id) = state.active().selection.single() else {
        ui.colored_label(
            Theme::TEXT_MUTED,
            format!("{} objects selected", state.active().selection.len()),
        );
        return;
    };
    let Some(frame) = state.active().document().frame(id).cloned() else {
        ui.colored_label(Theme::TEXT_MUTED, "No selection");
        return;
    };

    // Position is asked in document space and size in the frame's own space,
    // which is what each of them means. `bounds` is the frame's own box, so it
    // answers W and H directly — but it does not move when the frame does,
    // because a move is a change of placement.
    let origin = frame.corners()[0];
    let (mut x, mut y) = (origin.x, origin.y);
    let mut bounds = frame.bounds;
    let (mut moved, mut resized) = (false, false);

    ui.label("Position and size (pt)");
    egui::Grid::new("bounds").num_columns(2).show(ui, |ui| {
        moved |= scrub(ui, "X", &mut x);
        moved |= scrub(ui, "Y", &mut y);
        ui.end_row();
        resized |= scrub(ui, "W", &mut bounds.width);
        resized |= scrub(ui, "H", &mut bounds.height);
        ui.end_row();
    });

    if moved {
        // Translated in document space, so a turned frame goes where the
        // number says rather than off along its own axes.
        apply(
            state,
            Command::TranslateSelection {
                dx: x - origin.x,
                dy: y - origin.y,
            },
        );
    }
    if resized {
        apply(state, Command::SetBounds { id, bounds });
    }

    let mut degrees = frame.rotation_degrees();
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Rotation");
        if ui
            .add(egui::DragValue::new(&mut degrees).speed(0.5).suffix("°"))
            .changed()
        {
            apply(state, Command::SetRotation { id, degrees });
        }
    });

    ui.add_space(Theme::SPACING_MD);

    match &frame.kind {
        tessera_document::nodes::FrameKind::Text { story } => {
            ui.label("Text");
            let mut text = state
                .active()
                .document()
                .story(*story)
                .map(|s| s.text.clone())
                .unwrap_or_default();
            if ui.text_edit_multiline(&mut text).changed() {
                apply(state, Command::SetText { id, text });
            }
        }
        _ => {
            ui.label("Fill");
            let [r, g, b, a] = frame.fill.to_rgb_f32();
            let mut rgba = [r, g, b, a];
            if fill_picker(ui, &mut rgba) {
                apply(
                    state,
                    Command::SetFill {
                        id,
                        color: Color::Rgb {
                            r: rgba[0],
                            g: rgba[1],
                            b: rgba[2],
                            a: rgba[3],
                        },
                    },
                );
            }
        }
    }
}

fn fill_picker(ui: &mut Ui, rgba: &mut [f32; 4]) -> bool {
    let mut colour = egui::Rgba::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);
    let changed = egui::widgets::color_picker::color_edit_button_rgba(
        ui,
        &mut colour,
        egui::widgets::color_picker::Alpha::Opaque,
    )
    .changed();
    if changed {
        *rgba = [colour.r(), colour.g(), colour.b(), colour.a()];
    }
    changed
}

// --- document setup ----------------------------------------------------

/// The inspector with nothing selected: the document's own properties.
///
/// InDesign shows the same thing in the same place, and it is the one part of
/// its Properties panel worth keeping wholesale — with nothing selected, the
/// document *is* the selection.
pub fn document_setup(ui: &mut Ui, state: &mut TesseraApp) {
    let unit = state.prefs.unit;
    let mut setup = state.active().document().setup;
    let page = state.first_page_bounds();
    let (mut width, mut height) = (page.width, page.height);

    ui.label("Page");
    let mut resized = false;
    egui::Grid::new("page-size").num_columns(2).show(ui, |ui| {
        resized |= measure(ui, "W", &mut width, unit);
        resized |= measure(ui, "H", &mut height, unit);
        ui.end_row();
    });
    if resized {
        apply(state, Command::SetPageSize { width, height });
        return;
    }

    let mut changed = false;

    ui.add_space(Theme::SPACING_SM);
    changed |= ui
        .checkbox(&mut setup.facing_pages, "Facing pages")
        .changed();

    // The labels change with the binding, because the fields themselves mean
    // something different: with facing pages on, the wide margin is the one
    // against the spine and swaps sides between left-hand and right-hand
    // pages. Calling it "Left" then would be a lie on half the document.
    let (near, far) = if setup.facing_pages {
        ("Inside", "Outside")
    } else {
        ("Left", "Right")
    };

    ui.add_space(Theme::SPACING_MD);
    ui.label("Margins");
    egui::Grid::new("margins").num_columns(2).show(ui, |ui| {
        changed |= measure(ui, "Top", &mut setup.margins.top, unit);
        changed |= measure(ui, "Bottom", &mut setup.margins.bottom, unit);
        ui.end_row();
        changed |= measure(ui, near, &mut setup.margins.inside, unit);
        changed |= measure(ui, far, &mut setup.margins.outside, unit);
        ui.end_row();
    });

    ui.add_space(Theme::SPACING_MD);
    ui.label("Bleed");
    egui::Grid::new("bleed").num_columns(2).show(ui, |ui| {
        changed |= measure(ui, "Top", &mut setup.bleed.top, unit);
        changed |= measure(ui, "Bottom", &mut setup.bleed.bottom, unit);
        ui.end_row();
        changed |= measure(ui, "Left", &mut setup.bleed.left, unit);
        changed |= measure(ui, "Right", &mut setup.bleed.right, unit);
        ui.end_row();
    });

    ui.add_space(Theme::SPACING_MD);
    ui.label("Slug");
    egui::Grid::new("slug").num_columns(2).show(ui, |ui| {
        changed |= measure(ui, "Top", &mut setup.slug.top, unit);
        changed |= measure(ui, "Bottom", &mut setup.slug.bottom, unit);
        ui.end_row();
        changed |= measure(ui, "Left", &mut setup.slug.left, unit);
        changed |= measure(ui, "Right", &mut setup.slug.right, unit);
        ui.end_row();
    });

    if changed {
        // One command for the whole struct: a page-setup edit is one undo
        // entry, not one per field touched.
        apply(state, Command::SetDocumentSetup(setup));
    }

    ui.add_space(Theme::SPACING_LG);
    ui.colored_label(
        Theme::TEXT_MUTED,
        format!("Measurements in {}", unit_name(unit)),
    );
}

fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::Millimetres => "millimetres",
        Unit::Points => "points",
        Unit::Pixels => "pixels",
        Unit::Inches => "inches",
        Unit::Picas => "picas",
    }
}

/// A numeric field holding a measurement.
///
/// The document stores points; this shows and edits the user's preferred unit
/// and converts at the edge, which is the only place a conversion belongs.
fn measure(ui: &mut Ui, label: &str, points: &mut f64, unit: Unit) -> bool {
    let mut shown = unit.from_points(*points);
    let changed = ui
        .horizontal(|ui| {
            ui.colored_label(Theme::TEXT_MUTED, label);
            ui.add(
                egui::DragValue::new(&mut shown)
                    .speed(0.25)
                    .fixed_decimals(2)
                    .suffix(format!(" {}", unit.suffix())),
            )
            .changed()
        })
        .inner;
    if changed {
        *points = unit.to_points(shown);
    }
    changed
}

/// A numeric field that also scrubs when its label is dragged.
fn scrub(ui: &mut Ui, label: &str, value: &mut f64) -> bool {
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, label);
        ui.add(egui::DragValue::new(value).speed(0.5).fixed_decimals(1))
            .changed()
    })
    .inner
}

// --- status bar --------------------------------------------------------

pub fn status_bar(ui: &mut Ui, state: &TesseraApp) {
    ui.horizontal(|ui| {
        match &state.status {
            Some(s) if s.is_error => ui.colored_label(Theme::ERROR, &s.message),
            Some(s) => ui.colored_label(Theme::TEXT_MUTED, &s.message),
            None => ui.colored_label(Theme::TEXT_MUTED, state.active_tool.label()),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.colored_label(
                Theme::TEXT_MUTED,
                format!("{:.0}%", state.active().view.zoom * 100.0),
            );
        });
    });
}
