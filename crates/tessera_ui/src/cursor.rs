//! The pointer, painted rather than requested.
//!
//! `egui::CursorIcon` is a fixed vocabulary mapped onto whatever the platform
//! happens to ship. Windows has no grab cursor, so winit substitutes
//! `IDC_SIZEALL` — which is why asking for "grab" while rotating produced a
//! move cross instead. The set is also missing anything that means *rotate*.
//!
//! So the cursor is drawn, from the same Lucide geometry as the toolbar
//! ([`crate::icons`]), and the platform cursor is switched off over the
//! canvas. That buys a cursor that says exactly what a drag will do, turns to
//! follow a rotated frame's handles, and cannot silently change meaning on
//! another operating system.

use egui::{Painter, Pos2, Rect};

use crate::icons::{self, Icon};
use crate::theme::Theme;

/// What the pointer should look like right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cursor {
    pub icon: Icon,
    /// Clockwise degrees. Only [`Icon::Scale`] uses it, to point along a
    /// handle's own normal.
    pub rotation: f32,
}

impl Cursor {
    pub fn new(icon: Icon) -> Self {
        Self {
            icon,
            rotation: 0.0,
        }
    }

    pub fn turned(icon: Icon, rotation: f32) -> Self {
        Self { icon, rotation }
    }
}

/// The box the icon's 24-unit grid must occupy for its hotspot to land on
/// `at`.
///
/// The hotspot is a point in grid space, and the icon is rotated about the
/// box's centre — so the offset from centre to hotspot has to be rotated too,
/// then subtracted. Centring the box on the pointer instead would put an
/// arrow's tip several pixels down and to the right of what a click hits.
fn placement(at: Pos2, cursor: Cursor) -> Rect {
    let side = Theme::CURSOR_SIZE;
    let scale = side / 24.0;
    let (hx, hy) = cursor.icon.hotspot();
    let offset = egui::vec2((hx - 12.0) * scale, (hy - 12.0) * scale);
    let (sin, cos) = cursor.rotation.to_radians().sin_cos();
    let turned = egui::vec2(
        offset.x * cos - offset.y * sin,
        offset.x * sin + offset.y * cos,
    );
    Rect::from_center_size(at - turned, egui::vec2(side, side))
}

/// Paint `cursor` with its hotspot on `at`, in Lucide's own line weight.
///
/// `on_light` inverts it. The canvas has exactly two backgrounds — the white
/// page and the dark pasteboard — so the contrast that a casing stroke used to
/// buy is had by choosing a colour instead, and the cursor stays the same
/// single-weight line drawing as the icon in the toolbar.
pub fn paint(painter: &Painter, at: Pos2, cursor: Cursor, on_light: bool) {
    let color = if on_light {
        Theme::CURSOR_ON_LIGHT
    } else {
        Theme::CURSOR_ON_DARK
    };
    icons::paint_rotated(
        painter,
        placement(at, cursor),
        cursor.icon,
        color,
        cursor.rotation,
        1.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// Where the hotspot actually lands, given the box `placement` chose.
    fn hotspot_lands_at(at: Pos2, cursor: Cursor) -> Pos2 {
        let rect = placement(at, cursor);
        let scale = Theme::CURSOR_SIZE / 24.0;
        let (hx, hy) = cursor.icon.hotspot();
        let offset = egui::vec2((hx - 12.0) * scale, (hy - 12.0) * scale);
        let (sin, cos) = cursor.rotation.to_radians().sin_cos();
        rect.center()
            + egui::vec2(
                offset.x * cos - offset.y * sin,
                offset.x * sin + offset.y * cos,
            )
    }

    #[test]
    fn a_centred_icon_is_centred_on_the_pointer() {
        let at = Pos2::new(100.0, 50.0);
        let rect = placement(at, Cursor::new(Icon::Crosshair));
        assert!(close(rect.center().x, at.x));
        assert!(close(rect.center().y, at.y));
    }

    #[test]
    fn the_arrows_tip_lands_on_the_pointer_not_its_box() {
        // The whole point of a hotspot: the select arrow must click where its
        // tip is, not where the middle of its bounding box is.
        let at = Pos2::new(100.0, 50.0);
        let cursor = Cursor::new(Icon::Select);
        assert!(hotspot_lands_at(at, cursor).distance(at) < 1e-3);
        assert!(
            placement(at, cursor).center().distance(at) > 1.0,
            "the box must be offset, or there was no hotspot to honour"
        );
    }

    #[test]
    fn a_hotspot_survives_rotation() {
        let at = Pos2::new(10.0, 10.0);
        for degrees in [0.0, 45.0, 90.0, 180.0, 270.0] {
            let cursor = Cursor::turned(Icon::Select, degrees);
            assert!(
                hotspot_lands_at(at, cursor).distance(at) < 1e-3,
                "hotspot drifted at {degrees} degrees"
            );
        }
    }
}
