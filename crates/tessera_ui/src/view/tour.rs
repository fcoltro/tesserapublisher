//! The tour's card, and the ring around what it is talking about.

use egui::{Area, Frame, Order, Stroke};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// How wide the card is.
///
/// Narrow on purpose. A card as wide as the window is a paragraph, and a
/// paragraph is what somebody dismisses without reading; this is about eight
/// words to a line, which is what running text wants.
const CARD: f32 = 300.0;

/// How far the card sits from the thing it points at.
const GAP: f32 = 12.0;

/// How tall to assume the card is when placing it.
///
/// Guessed at, because the real height is not known until it has been laid out —
/// and by then it has been placed. Generous rather than tight: too much room
/// pushes a card slightly further from an edge than it needed to be, while too
/// little pushes its buttons off the bottom.
const TALL: f32 = 220.0;

/// Show the current step, if the tour is running.
///
/// Called last, after every panel has said where it is. Anything earlier would
/// be reading marks from the previous frame, and a card one frame behind the
/// panel it points at is a card that lags visibly while a rail is dragged.
pub fn show(ui: &mut egui::Ui, state: &mut TesseraApp) {
    // A first run opens New Document, and the tour waits for it: pointing at a
    // rail behind a modal is pointing at something nobody can look at or click.
    if state.tour.start_if_clear(!state.new_document.open) {
        // Recorded the moment it starts rather than when it finishes, so a tour
        // somebody quit by closing the window does not come back tomorrow.
        state.prefs.tour_seen = true;
        crate::prefs::remember(state);
    }

    let Some((step, at)) = state.tour.showing() else {
        return;
    };
    let counted = state.tour.counted();

    // The ring, on a layer above the panels. Painted rather than drawn into the
    // panel, because the thing being pointed at belongs to somebody else and
    // this must not disturb its layout.
    let ring = ui.ctx().layer_painter(egui::LayerId::new(
        Order::Foreground,
        egui::Id::new("tour-ring"),
    ));
    ring.rect_stroke(
        at.shrink(1.0),
        Theme::RADIUS,
        Stroke::new(2.0, Theme::accent()),
        egui::StrokeKind::Inside,
    );

    let mut next = false;
    let mut done = false;

    Area::new(egui::Id::new("tour-card"))
        .order(Order::Foreground)
        .fixed_pos(beside(at, ui.max_rect()))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(Theme::panel_bg_solid())
                .stroke(Stroke::new(1.0, Theme::accent()))
                .corner_radius(Theme::RADIUS)
                .inner_margin(Theme::SPACING_LG)
                .show(ui, |ui| {
                    ui.set_width(CARD);
                    ui.label(
                        egui::RichText::new(step.title)
                            .size(Theme::TYPE_LG)
                            .color(Theme::text_primary())
                            .strong(),
                    );
                    ui.add_space(Theme::SPACING_SM);
                    ui.label(
                        egui::RichText::new(step.body)
                            .size(Theme::TYPE_MD)
                            .color(Theme::text_primary()),
                    );
                    ui.add_space(Theme::SPACING_MD);
                    ui.horizontal(|ui| {
                        if let Some((this, all)) = counted {
                            ui.label(
                                egui::RichText::new(format!("{this} of {all}"))
                                    .size(Theme::TYPE_SM)
                                    .color(Theme::text_muted()),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // The last step says "Done", because "Next"
                            // on the last card promises another one.
                            let last = counted.is_some_and(|(this, all)| this == all);
                            if ui.button(if last { "Done" } else { "Next" }).clicked() {
                                next = true;
                            }
                            // Always available. A tour somebody cannot leave
                            // is a tour they resent, and this one is five
                            // cards about where things are — not a licence
                            // agreement.
                            if ui.button("Skip the tour").clicked() {
                                done = true;
                            }
                        });
                    });
                });
        });

    if done {
        state.tour.end();
    } else if next {
        state.tour.next();
    }
}

/// Where to put the card so that it does not cover what it points at.
///
/// Beside a tall spot, below or above a wide one, and always inside the window:
/// a card half off the edge takes its buttons with it, and the tour becomes
/// something somebody has to resize the window to escape.
fn beside(at: egui::Rect, window: egui::Rect) -> egui::Pos2 {
    let wide = at.width() > at.height();
    let mut spot = if wide {
        // Under a bar, unless the bar is at the bottom of the window.
        let below = at.bottom() + GAP;
        let above = at.top() - GAP - TALL;
        egui::pos2(
            at.left() + GAP,
            if below + TALL <= window.bottom() {
                below
            } else {
                above
            },
        )
    } else {
        // Beside a strip, on whichever side has room.
        let right = at.right() + GAP;
        let left = at.left() - GAP - CARD;
        egui::pos2(
            if right + CARD <= window.right() {
                right
            } else {
                left
            },
            at.top() + GAP,
        )
    };

    // Clamped last, so neither branch above can put the buttons off-screen.
    // `max` after `min` deliberately: on a window smaller than the card the
    // top-left corner is what has to stay visible.
    spot.x = spot
        .x
        .min(window.right() - CARD - GAP)
        .max(window.left() + GAP);
    spot.y = spot
        .y
        .min(window.bottom() - TALL - GAP)
        .max(window.top() + GAP);
    spot
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 840.0))
    }

    /// Whatever `beside` returns, a card placed there has to fit.
    fn fits(spot: egui::Pos2) {
        let window = window();
        assert!(
            spot.x >= window.left() && spot.x + CARD <= window.right(),
            "the card is off the side at {spot:?}"
        );
        assert!(
            spot.y >= window.top() && spot.y + TALL <= window.bottom(),
            "the card is off the bottom at {spot:?}"
        );
    }

    #[test]
    fn a_card_beside_the_left_strip_goes_to_its_right() {
        let tools = egui::Rect::from_min_size(egui::pos2(0.0, 60.0), egui::vec2(48.0, 700.0));
        let spot = beside(tools, window());
        assert!(spot.x > tools.right(), "the card covers the tools");
        fits(spot);
    }

    #[test]
    fn a_card_beside_the_right_rail_goes_to_its_left() {
        // **The case the first version got wrong.** A rail at the right edge has
        // no room to its right, and a card placed there is a card whose buttons
        // are outside the window.
        let rail = egui::Rect::from_min_size(egui::pos2(1000.0, 60.0), egui::vec2(280.0, 700.0));
        let spot = beside(rail, window());
        assert!(spot.x + CARD <= rail.left(), "the card covers the rail");
        fits(spot);
    }

    #[test]
    fn a_card_under_the_status_bar_goes_above_it() {
        let status = egui::Rect::from_min_size(egui::pos2(0.0, 816.0), egui::vec2(1280.0, 24.0));
        let spot = beside(status, window());
        assert!(
            spot.y + TALL <= status.top(),
            "the card covers the status bar"
        );
        fits(spot);
    }

    #[test]
    fn a_card_under_the_control_bar_goes_below_it() {
        let control = egui::Rect::from_min_size(egui::pos2(0.0, 30.0), egui::vec2(1280.0, 32.0));
        let spot = beside(control, window());
        assert!(
            spot.y >= control.bottom(),
            "the card covers the control bar"
        );
        fits(spot);
    }

    #[test]
    fn a_card_in_a_window_barely_larger_than_itself_still_shows_its_corner() {
        // A window this small is not one anybody works in, but the tour must not
        // put its buttons somewhere unreachable if somebody drags the edge in.
        let cramped = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 200.0));
        let strip = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(48.0, 200.0));
        let spot = beside(strip, cramped);
        assert!(spot.x >= cramped.left(), "{spot:?}");
        assert!(spot.y >= cramped.top(), "{spot:?}");
    }
}
