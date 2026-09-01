//! The tool strip, the inspector and the status bar.

use egui::{Sense, Ui, Vec2};
use tessera_color::Color;

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

    if state.selection.is_empty() {
        ui.colored_label(Theme::TEXT_MUTED, "No selection");
        return;
    }

    // Geometry fields edit one frame. With several selected there is no single
    // value to show, and silently editing only the first would be worse than
    // saying so.
    let Some(id) = state.selection.single() else {
        ui.colored_label(
            Theme::TEXT_MUTED,
            format!("{} objects selected", state.selection.len()),
        );
        return;
    };
    let Some(frame) = state.document.frame(id).cloned() else {
        ui.colored_label(Theme::TEXT_MUTED, "No selection");
        return;
    };

    let mut bounds = frame.bounds;
    let mut changed = false;

    ui.label("Position and size (pt)");
    egui::Grid::new("bounds").num_columns(2).show(ui, |ui| {
        changed |= scrub(ui, "X", &mut bounds.x);
        changed |= scrub(ui, "Y", &mut bounds.y);
        ui.end_row();
        changed |= scrub(ui, "W", &mut bounds.width);
        changed |= scrub(ui, "H", &mut bounds.height);
        ui.end_row();
    });

    if changed {
        apply(state, Command::SetBounds { id, bounds });
    }

    let mut degrees = frame.rotation;
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
                .document
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
                format!("{:.0}%", state.view.zoom * 100.0),
            );
        });
    });
}
