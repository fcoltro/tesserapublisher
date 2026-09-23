//! The pointer, painted rather than requested.
//!
//! `egui::CursorIcon` is a fixed vocabulary mapped onto whatever the platform
//! happens to ship. Windows has no grab cursor, so winit substitutes
//! `IDC_SIZEALL` — which is why asking for "grab" while rotating produced a
//! move cross instead. The set is also missing anything that means *rotate*.
//!
//! So the cursor is drawn, from the same Spectrum pictures as the toolbar
//! ([`crate::icons`]), and the platform cursor is switched off over the
//! canvas. That buys a cursor that says exactly what a drag will do, turns to
//! follow a rotated frame's handles, and cannot silently change meaning on
//! another operating system.
//!
//! And it is drawn in the **opposite of whatever it is over**: the mesh goes
//! through [`crate::view::invert_host`], a blend that makes every pixel one
//! minus what was there. A first cut chose black on the page and white on the
//! pasteboard, and vanished over a black rectangle on the page.

use egui::epaint::Mesh;
use egui::{Color32, Pos2, Rect};

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

/// The box the icon's grid must occupy for its hotspot to land on
/// `at`.
///
/// The hotspot is a point in grid space, and the icon is rotated about the
/// box's centre — so the offset from centre to hotspot has to be rotated too,
/// then subtracted. Centring the box on the pointer instead would put an
/// arrow's tip several pixels down and to the right of what a click hits.
fn placement(at: Pos2, cursor: Cursor) -> Rect {
    let side = Theme::CURSOR_SIZE;
    let grid = cursor.icon.grid();
    let scale = side / grid;
    let (hx, hy) = cursor.icon.hotspot();
    let offset = egui::vec2((hx - grid / 2.0) * scale, (hy - grid / 2.0) * scale);
    let (sin, cos) = cursor.rotation.to_radians().sin_cos();
    let turned = egui::vec2(
        offset.x * cos - offset.y * sin,
        offset.x * sin + offset.y * cos,
    );
    Rect::from_center_size(at - turned, egui::vec2(side, side))
}

/// `cursor` with its hotspot on `at`, as the mesh the inverting pass draws:
/// one square per pixel the picture covers.
///
/// White at full coverage: through the blend, white *is* "the opposite", and
/// the feathered edge's alpha is how much of the pixel turns over.
pub fn mesh(at: Pos2, cursor: Cursor, pixels_per_point: f32) -> Mesh {
    icons::coverage_mesh(
        placement(at, cursor),
        cursor.icon,
        Color32::WHITE,
        cursor.rotation,
        pixels_per_point,
    )
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
        let grid = cursor.icon.grid();
        let scale = Theme::CURSOR_SIZE / grid;
        let (hx, hy) = cursor.icon.hotspot();
        let offset = egui::vec2((hx - grid / 2.0) * scale, (hy - grid / 2.0) * scale);
        let (sin, cos) = cursor.rotation.to_radians().sin_cos();
        rect.center()
            + egui::vec2(
                offset.x * cos - offset.y * sin,
                offset.x * sin + offset.y * cos,
            )
    }

    #[test]
    fn the_mesh_is_white_triangles_around_the_pointer() {
        // What the inverting pass needs: white so the blend makes the
        // opposite, and triangles where the pointer is.
        let at = Pos2::new(100.0, 50.0);
        let mesh = mesh(at, Cursor::new(Icon::Select), 1.0);
        assert!(!mesh.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(
            mesh.vertices
                .iter()
                .all(|v| v.color == Color32::WHITE || v.color.a() < 255),
            "solid vertices are white; only the feathered edge fades"
        );
        let bounds = mesh.calc_bounds();
        assert!(bounds.contains(at), "the hotspot is inside the drawing");
        assert!(bounds.width() <= Theme::CURSOR_SIZE * 1.5);
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
