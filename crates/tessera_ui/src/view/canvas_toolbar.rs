//! The spatial verbs, beside the object they act on.
//!
//! This is D2. Values — a width, a stroke weight, an angle — are read and
//! compared, so they belong in a stable list on the right. Actions whose
//! result is a change of position or orientation are judged by looking at the
//! object, so the control belongs next to the object.
//!
//! **Nothing here appears in the rail**, so the two surfaces cannot disagree
//! about anything. That is what keeps this from becoming InDesign's three
//! separate routes to a rotation.

use egui::{Pos2, Rect, Ui, Vec2};

use crate::align::{AlignTo, Edge};
use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// How far the toolbar floats from the selection, in screen points.
const GAP: f32 = 10.0;

/// Where the toolbar sits: below the selection when there is room, above it
/// otherwise, and always inside the viewport.
///
/// Placement is a pure function so the awkward cases — a selection at the
/// bottom of the window, one against the right edge — are testable without a
/// window to put them in.
pub fn place(selection: Rect, toolbar: Vec2, viewport: Rect) -> Pos2 {
    let below = selection.max.y + GAP;
    let y = if below + toolbar.y <= viewport.max.y {
        below
    } else {
        // No room underneath, so it goes above rather than off the bottom.
        (selection.min.y - GAP - toolbar.y).max(viewport.min.y)
    };

    let centred = selection.center().x - toolbar.x / 2.0;
    let x = centred.clamp(
        viewport.min.x,
        (viewport.max.x - toolbar.x).max(viewport.min.x),
    );

    Pos2::new(x, y)
}

/// Draw the toolbar for the current selection.
///
/// `selection` is the selection's box on screen. Does nothing for a selection
/// of fewer than two objects: every verb here is about the relationship
/// between objects, and a toolbar of disabled buttons is noise.
pub fn show(ui: &mut Ui, state: &mut TesseraApp, selection: Rect, viewport: Rect) {
    if state.active().selection.len() < 2 {
        return;
    }

    // The buttons carry short labels rather than icons for now. The Lucide
    // glyphs for align and distribute arrive with C13, which grows the icon
    // set; fabricating path data here that nobody can check would be worse
    // than a label that is merely plain.
    const BUTTONS: &[(&str, &str, Verb)] = &[
        ("L", "Align left edges", Verb::Align(Edge::Left)),
        ("C", "Align horizontal centres", Verb::Align(Edge::HCentre)),
        ("R", "Align right edges", Verb::Align(Edge::Right)),
        ("T", "Align top edges", Verb::Align(Edge::Top)),
        ("M", "Align vertical centres", Verb::Align(Edge::VCentre)),
        ("B", "Align bottom edges", Verb::Align(Edge::Bottom)),
        ("↔", "Distribute horizontally", Verb::DistributeH),
        ("↕", "Distribute vertically", Verb::DistributeV),
    ];

    let size = Vec2::new(BUTTONS.len() as f32 * 26.0 + 12.0, 28.0);
    let at = place(selection, size, viewport);

    let mut chosen = None;
    egui::Area::new(ui.id().with("canvas-toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(Theme::PANEL_BG)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (label, tip, verb) in BUTTONS {
                            if ui.small_button(*label).on_hover_text(*tip).clicked() {
                                chosen = Some(*verb);
                            }
                        }
                    });
                });
        });

    match chosen {
        Some(Verb::Align(edge)) => apply(
            state,
            Command::Align {
                edge,
                to: AlignTo::Selection,
            },
        ),
        Some(Verb::DistributeH) => apply(
            state,
            Command::Distribute(tessera_document::nodes::Axis::Horizontal),
        ),
        Some(Verb::DistributeV) => apply(
            state,
            Command::Distribute(tessera_document::nodes::Axis::Vertical),
        ),
        None => {}
    }
}

#[derive(Debug, Clone, Copy)]
enum Verb {
    Align(Edge),
    DistributeH,
    DistributeV,
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn viewport() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))
    }

    #[test]
    fn the_toolbar_goes_below_the_selection_when_there_is_room() {
        let selection = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 100.0));
        let at = place(selection, vec2(180.0, 28.0), viewport());
        assert!(at.y > selection.max.y, "it sat over the object");
    }

    #[test]
    fn the_toolbar_goes_above_when_there_is_no_room_below() {
        let selection = Rect::from_min_size(pos2(100.0, 540.0), vec2(200.0, 50.0));
        let at = place(selection, vec2(180.0, 28.0), viewport());
        assert!(at.y < selection.min.y, "it fell off the bottom");
    }

    #[test]
    fn the_toolbar_stays_inside_the_viewport_horizontally() {
        let selection = Rect::from_min_size(pos2(760.0, 100.0), vec2(30.0, 30.0));
        let at = place(selection, vec2(180.0, 28.0), viewport());
        assert!(at.x >= viewport().min.x);
        assert!(
            at.x + 180.0 <= viewport().max.x,
            "it ran off the right edge"
        );
    }

    #[test]
    fn a_toolbar_wider_than_the_viewport_still_starts_inside_it() {
        // Degenerate, but a negative clamp range panics rather than
        // misplacing something, so it is worth pinning.
        let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 600.0));
        let selection = Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 20.0));
        let at = place(selection, vec2(400.0, 28.0), narrow);
        assert!(at.x >= narrow.min.x);
    }

    #[test]
    fn the_toolbar_is_centred_on_the_selection_when_it_fits() {
        let selection = Rect::from_min_size(pos2(300.0, 100.0), vec2(200.0, 100.0));
        let at = place(selection, vec2(180.0, 28.0), viewport());
        assert!((at.x + 90.0 - selection.center().x).abs() < 1e-3);
    }
}
