//! Pan and zoom.
//!
//! The camera lives in [`ViewTransform`], which is owned by the viewport and
//! never by the document: panning is not an edit, so it neither marks the
//! document dirty nor lands in the undo stack.

use tessera_geometry::{DocRect, ScreenPoint, ViewTransform};

const MIN_ZOOM: f64 = 0.02;
const MAX_ZOOM: f64 = 64.0;
/// Leaves a visible pasteboard margin around a fitted page.
const FIT_MARGIN: f64 = 0.9;

/// Scale about a screen point, keeping the document point under it fixed.
///
/// This is what makes wheel-zoom feel anchored: the pixel under the cursor
/// must still show the same part of the document afterwards.
pub fn zoom_about(view: &mut ViewTransform, cursor: ScreenPoint, factor: f64) {
    let anchor = view.screen_to_doc(cursor);
    view.zoom = (view.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    let after = view.screen_to_doc(cursor);
    view.pan.x += anchor.x - after.x;
    view.pan.y += anchor.y - after.y;
}

/// Fit the page into a viewport of `width` x `height` screen pixels.
pub fn zoom_to_fit(view: &mut ViewTransform, page: DocRect, width: f32, height: f32) {
    if width <= 0.0 || height <= 0.0 || page.width <= 0.0 || page.height <= 0.0 {
        return;
    }
    let sx = f64::from(width) / page.width;
    let sy = f64::from(height) / page.height;
    view.zoom = (sx.min(sy) * FIT_MARGIN).clamp(MIN_ZOOM, MAX_ZOOM);
    view.pan.x = page.x - (f64::from(width) / view.zoom - page.width) / 2.0;
    view.pan.y = page.y - (f64::from(height) / view.zoom - page.height) / 2.0;
}

/// Fit an arbitrary rectangle, which is what the zoom tool's drag asks for.
///
/// `zoom_to_fit` in terms of any rectangle rather than a page. Written as one
/// call to the other rather than the same four lines twice: the margin, the
/// clamp and the centring are one fact about how this application frames
/// things, and two copies of it drift.
pub fn zoom_to(view: &mut ViewTransform, area: DocRect, width: f32, height: f32) {
    zoom_to_fit(view, area, width, height);
}

/// How far a scrubby-zoom drag has zoomed, as a factor of the zoom it
/// started at: double for every [`SCRUB_DOUBLING`] pixels to the right, half
/// for as many to the left — Illustrator's feel, where the same distance
/// always means the same change however far in somebody already is.
pub fn scrub_factor(dx: f32) -> f64 {
    2f64.powf(f64::from(dx) / SCRUB_DOUBLING)
}

/// Screen pixels of horizontal drag that double the zoom.
pub const SCRUB_DOUBLING: f64 = 120.0;

/// Set the zoom to `zoom`, keeping the document point under `about` fixed.
pub fn zoom_to_level(view: &mut ViewTransform, about: ScreenPoint, zoom: f64) {
    let factor = zoom / view.zoom.max(f64::MIN_POSITIVE);
    zoom_about(view, about, factor);
}

pub fn pan_by(view: &mut ViewTransform, screen_dx: f32, screen_dy: f32) {
    view.pan.x -= f64::from(screen_dx) / view.zoom;
    view.pan.y -= f64::from(screen_dy) / view.zoom;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocPoint;

    fn page() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 612.0,
            height: 792.0,
        }
    }

    #[test]
    fn zooming_about_a_point_keeps_that_document_point_under_the_cursor() {
        let mut view = ViewTransform {
            pan: DocPoint::ZERO,
            zoom: 1.0,
        };
        let cursor = ScreenPoint { x: 300.0, y: 200.0 };
        let before = view.screen_to_doc(cursor);

        zoom_about(&mut view, cursor, 1.5);

        let after = view.screen_to_doc(cursor);
        assert!((before.x - after.x).abs() < 0.001);
        assert!((before.y - after.y).abs() < 0.001);
    }

    #[test]
    fn zoom_is_clamped_at_both_ends() {
        let mut view = ViewTransform::default();
        let origin = ScreenPoint { x: 0.0, y: 0.0 };
        for _ in 0..200 {
            zoom_about(&mut view, origin, 2.0);
        }
        assert!(view.zoom <= MAX_ZOOM);
        for _ in 0..400 {
            zoom_about(&mut view, origin, 0.5);
        }
        assert!(view.zoom >= MIN_ZOOM);
    }

    #[test]
    fn zoom_to_fit_binds_on_the_tighter_axis() {
        let mut view = ViewTransform::default();
        zoom_to_fit(&mut view, page(), 1000.0, 800.0);
        // Height binds: 800/792 is smaller than 1000/612.
        assert!(view.zoom < 800.0 / 792.0);
        assert!(view.zoom > 0.5);
    }

    #[test]
    fn a_fitted_page_is_centred() {
        let mut view = ViewTransform::default();
        zoom_to_fit(&mut view, page(), 1000.0, 800.0);

        let top_left = view.doc_to_screen(DocPoint { x: 0.0, y: 0.0 });
        let bottom_right = view.doc_to_screen(DocPoint {
            x: page().width,
            y: page().height,
        });
        let left_margin = top_left.x;
        let right_margin = 1000.0 - bottom_right.x;
        assert!(
            (left_margin - right_margin).abs() < 1.0,
            "left {left_margin} right {right_margin}"
        );
    }

    #[test]
    fn fitting_into_a_zero_sized_viewport_is_ignored_rather_than_dividing_by_zero() {
        let mut view = ViewTransform::default();
        let before = view.zoom;
        zoom_to_fit(&mut view, page(), 0.0, 0.0);
        assert_eq!(view.zoom, before);
    }

    #[test]
    fn panning_moves_the_document_opposite_the_drag() {
        let mut view = ViewTransform::default();
        pan_by(&mut view, 10.0, 0.0);
        // Dragging right moves the camera left, so content follows the cursor.
        assert!(view.pan.x < 0.0);
    }

    #[test]
    fn a_scrub_doubles_to_the_right_and_halves_to_the_left() {
        assert!((scrub_factor(0.0) - 1.0).abs() < 1e-9);
        assert!((scrub_factor(120.0) - 2.0).abs() < 1e-9);
        assert!((scrub_factor(-120.0) - 0.5).abs() < 1e-9);
        // The same distance is the same change: two halves make the whole.
        assert!((scrub_factor(60.0) * scrub_factor(60.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn zooming_to_a_level_keeps_the_point_under_the_pointer() {
        let mut view = ViewTransform::default();
        let about = ScreenPoint { x: 200.0, y: 100.0 };
        let before = view.screen_to_doc(about);
        zoom_to_level(&mut view, about, 3.0);
        assert!((view.zoom - 3.0).abs() < 1e-9);
        let after = view.screen_to_doc(about);
        assert!((before.x - after.x).abs() < 1e-9 && (before.y - after.y).abs() < 1e-9);
    }
}
