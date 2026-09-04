//! The rulers, and the unit selector at their corner.

use egui::{Rect, Ui};
use tessera_geometry::{DocPoint, Unit};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// How thick a ruler is, in screen points.
pub const THICKNESS: f32 = 20.0;

/// The narrowest a labelled tick may be drawn, in screen pixels.
///
/// Labels have to fit between the ticks, so this is a good deal larger than
/// the ticks themselves would need.
const MIN_LABEL_GAP: f64 = 46.0;

/// The gap between labelled ticks, in **points**.
///
/// Chosen from a 1–2–5 ladder of multiples of the unit, so the numbers a
/// person reads are round ones — 10, 20, 50 — rather than whatever happened
/// to fit the available width.
pub fn tick_spacing(unit: Unit, zoom: f64) -> f64 {
    let zoom = zoom.max(f64::EPSILON);
    let base = unit.points_per();

    for decade in -4..=9 {
        for multiple in [1.0, 2.0, 5.0] {
            let step = base * multiple * 10f64.powi(decade);
            if step * zoom >= MIN_LABEL_GAP {
                return step;
            }
        }
    }
    // Unreachable at any zoom a person can set; a positive answer still beats
    // returning zero and dividing by it downstream.
    base * 10f64.powi(9)
}

/// Paint both rulers, given the strips reserved for them and the canvas they
/// measure.
pub fn paint(ui: &Ui, state: &TesseraApp, canvas: Rect, horizontal: Rect, vertical: Rect) {
    let unit = state.prefs.unit;
    let view = state.active().view;
    let zoom = view.zoom;
    let step = tick_spacing(unit, zoom);

    // The zero point is the first page's top-left corner: the origin a
    // measurement in a document is actually taken from.
    let origin = state.first_page_bounds();

    for (strip, is_horizontal) in [(horizontal, true), (vertical, false)] {
        let painter = ui.painter_at(strip);
        painter.rect_filled(strip, 0.0, Theme::PANEL_BG_ALT);

        let (from, to) = if is_horizontal {
            (canvas.min.x, canvas.max.x)
        } else {
            (canvas.min.y, canvas.max.y)
        };

        // The document coordinate at each end of the visible canvas, so only
        // the ticks that can be seen are considered.
        let doc_at = |screen: f32| {
            let local = tessera_geometry::ScreenPoint {
                x: if is_horizontal {
                    screen - canvas.min.x
                } else {
                    0.0
                },
                y: if is_horizontal {
                    0.0
                } else {
                    screen - canvas.min.y
                },
            };
            let p = view.screen_to_doc(local);
            if is_horizontal { p.x } else { p.y }
        };

        let start = doc_at(from);
        let end = doc_at(to);
        let zero = if is_horizontal { origin.x } else { origin.y };

        let first = ((start - zero) / step).floor() as i64;
        let last = ((end - zero) / step).ceil() as i64;

        for i in first..=last {
            let doc = zero + step * i as f64;
            let point = if is_horizontal {
                DocPoint { x: doc, y: 0.0 }
            } else {
                DocPoint { x: 0.0, y: doc }
            };
            let s = view.doc_to_screen(point);
            let at = if is_horizontal {
                canvas.min.x + s.x
            } else {
                canvas.min.y + s.y
            };
            if !(from..=to).contains(&at) {
                continue;
            }

            let hair = egui::Stroke::new(1.0, Theme::TEXT_MUTED);
            if is_horizontal {
                painter.line_segment(
                    [
                        egui::pos2(at, strip.max.y - 6.0),
                        egui::pos2(at, strip.max.y),
                    ],
                    hair,
                );
                painter.text(
                    egui::pos2(at + 2.0, strip.min.y),
                    egui::Align2::LEFT_TOP,
                    format!("{:.0}", unit.from_points(doc - zero)),
                    egui::FontId::proportional(9.0),
                    Theme::TEXT_MUTED,
                );
            } else {
                painter.line_segment(
                    [
                        egui::pos2(strip.max.x - 6.0, at),
                        egui::pos2(strip.max.x, at),
                    ],
                    hair,
                );
                painter.text(
                    egui::pos2(strip.min.x + 1.0, at + 2.0),
                    egui::Align2::LEFT_TOP,
                    format!("{:.0}", unit.from_points(doc - zero)),
                    egui::FontId::proportional(9.0),
                    Theme::TEXT_MUTED,
                );
            }
        }
    }
}

/// Let a guide be pulled off a ruler.
///
/// The top ruler yields a horizontal guide and the left ruler a vertical one:
/// you drag the line out in the direction it will lie across.
///
/// Nothing is written to the document until the drag ends, so an abandoned
/// drag costs neither a guide nor an undo entry.
pub fn drag_out(ui: &Ui, state: &mut TesseraApp, canvas: Rect, across: Rect, down: Rect) {
    use tessera_document::nodes::{Axis, Guide};

    let view = state.active().view;
    let doc_at = |pos: egui::Pos2| {
        view.screen_to_doc(tessera_geometry::ScreenPoint {
            x: pos.x - canvas.min.x,
            y: pos.y - canvas.min.y,
        })
    };

    let pointer = ui.ctx().pointer_latest_pos();
    let down_now = ui.ctx().input(|i| i.pointer.primary_down());
    let released = ui.ctx().input(|i| i.pointer.primary_released());

    if state.guide_drag.is_none()
        && down_now
        && let Some(pos) = pointer
    {
        if across.contains(pos) {
            state.guide_drag = Some((Axis::Horizontal, doc_at(pos).y));
        } else if down.contains(pos) {
            state.guide_drag = Some((Axis::Vertical, doc_at(pos).x));
        }
    }

    let Some((axis, _)) = state.guide_drag else {
        return;
    };

    if let Some(pos) = pointer {
        let at = doc_at(pos);
        let position = match axis {
            Axis::Horizontal => at.y,
            Axis::Vertical => at.x,
        };
        state.guide_drag = Some((axis, position));

        // The line under the pointer, so the drag is visible before it lands.
        let painter = ui.painter_at(canvas);
        let hair = egui::Stroke::new(1.0, Theme::GUIDE);
        let _drawn = match axis {
            Axis::Horizontal => painter.line_segment(
                [
                    egui::pos2(canvas.min.x, pos.y),
                    egui::pos2(canvas.max.x, pos.y),
                ],
                hair,
            ),
            Axis::Vertical => painter.line_segment(
                [
                    egui::pos2(pos.x, canvas.min.y),
                    egui::pos2(pos.x, canvas.max.y),
                ],
                hair,
            ),
        };
    }

    if released {
        let (axis, position) = state.guide_drag.take().expect("just matched");
        // Dropped back on a ruler rather than on the page: the gesture is
        // cancelled, which is how a guide is thrown away.
        let landed_on_canvas = pointer.is_some_and(|p| canvas.contains(p));
        // Bound before applying: the lookup borrows `state`, and the command
        // needs it mutably.
        let spread = state.active().document().spread_ids().next();
        if landed_on_canvas && let Some(spread) = spread {
            crate::command::apply(
                state,
                crate::command::Command::AddGuide {
                    spread,
                    guide: Guide {
                        axis,
                        position,
                        locked: false,
                    },
                },
            );
        }
    }
}

/// The unit selector, at the corner where the two rulers meet.
///
/// Writes the preference **and saves it**, so the choice survives a restart —
/// which is what phase A's preferences store was built for.
pub fn unit_selector(ui: &mut Ui, state: &mut TesseraApp) {
    let current = state.prefs.unit;
    let mut chosen = current;

    egui::ComboBox::from_id_salt("ruler-unit")
        .width(THICKNESS * 2.4)
        .selected_text(current.suffix())
        .show_ui(ui, |ui| {
            for unit in Unit::ALL {
                if ui
                    .selectable_label(unit == current, unit.suffix())
                    .clicked()
                {
                    chosen = unit;
                }
            }
        });

    if chosen != current {
        state.prefs.unit = chosen;
        if let Some(path) = crate::prefs::Preferences::path()
            && let Err(error) = state.prefs.save_to(&path)
        {
            state.status = Some(crate::app::Status::error(format!(
                "Could not save preferences: {error}"
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_never_get_closer_than_a_readable_gap() {
        for zoom in [0.05, 0.25, 1.0, 4.0, 32.0] {
            for unit in Unit::ALL {
                let step = tick_spacing(unit, zoom);
                assert!(step > 0.0, "{unit:?} at {zoom} gave {step}");
                assert!(
                    step * zoom >= MIN_LABEL_GAP,
                    "{unit:?} at {zoom} would draw labels {} px apart",
                    step * zoom
                );
            }
        }
    }

    #[test]
    fn zooming_in_subdivides_rather_than_multiplying() {
        // More zoom must never mean a coarser ruler.
        for unit in Unit::ALL {
            let coarse = tick_spacing(unit, 0.25);
            let fine = tick_spacing(unit, 4.0);
            assert!(fine <= coarse, "{unit:?} got coarser when zoomed in");
        }
    }

    #[test]
    fn a_tick_is_a_round_number_of_the_unit() {
        // 1, 2 or 5 times a power of ten — the numbers a person reads.
        for unit in Unit::ALL {
            for zoom in [0.1, 0.5, 1.0, 3.0, 10.0] {
                let in_units = tick_spacing(unit, zoom) / unit.points_per();
                let normalised = in_units / 10f64.powf(in_units.log10().floor());
                assert!(
                    [1.0, 2.0, 5.0]
                        .iter()
                        .any(|m| (normalised - m).abs() < 1e-6),
                    "{unit:?} at {zoom} gave {in_units} units per tick"
                );
            }
        }
    }

    #[test]
    fn a_zoom_of_zero_does_not_divide_by_it() {
        // The viewport can report this for a frame before it has sized itself.
        assert!(tick_spacing(Unit::Millimetres, 0.0).is_finite());
    }
}
