//! Placing a frame-local path inside its frame.
//!
//! A path is stored in its own coordinates so that moving its frame does not
//! rewrite its geometry. Everything that wants to know where the path really
//! is — the renderer, the PDF writer, hit testing — has to answer the same
//! question, so it is answered once, here.

use tessera_geometry::DocRect;

/// Map a frame-local path onto its frame's bounds.
///
/// Without this a line or a pen-drawn curve would keep its original size while
/// its frame was resized: the box would move and the geometry would not.
/// Doing it at the point of use rather than when the bounds change means
/// *every* route to a new size works — the handles, the inspector's fields,
/// and anything added later.
///
/// The result stays **frame-local**: the origin is the frame's top-left, not
/// the document's. Every consumer already translates by the frame's origin,
/// and having this do it too would move every path twice.
///
/// An axis with no extent — a perfectly horizontal line — is scaled on the
/// other axis only, since there is nothing to scale.
pub fn fit_to_bounds(path: &kurbo::BezPath, bounds: DocRect) -> kurbo::BezPath {
    use kurbo::Shape as _;

    let b = path.bounding_box();
    if b.width() <= f64::EPSILON && b.height() <= f64::EPSILON {
        return path.clone();
    }

    let sx = if b.width() > f64::EPSILON {
        bounds.width / b.width()
    } else {
        1.0
    };
    let sy = if b.height() > f64::EPSILON {
        bounds.height / b.height()
    } else {
        1.0
    };

    let mut out = path.clone();
    out.apply_affine(
        kurbo::Affine::scale_non_uniform(sx, sy) * kurbo::Affine::translate((-b.x0, -b.y0)),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape as _;

    fn diagonal() -> kurbo::BezPath {
        let mut p = kurbo::BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 10.0));
        p
    }

    #[test]
    fn a_path_fills_the_bounds_it_is_given() {
        let out = fit_to_bounds(
            &diagonal(),
            DocRect {
                x: 100.0,
                y: 50.0,
                width: 20.0,
                height: 40.0,
            },
        );
        let b = out.bounding_box();
        // Frame-local: sized to the bounds, but still anchored at the origin.
        assert!((b.x0 - 0.0).abs() < 1e-9, "x0 was {}", b.x0);
        assert!((b.y0 - 0.0).abs() < 1e-9, "y0 was {}", b.y0);
        assert!((b.width() - 20.0).abs() < 1e-9);
        assert!((b.height() - 40.0).abs() < 1e-9);
    }

    #[test]
    fn a_flat_axis_is_moved_rather_than_stretched() {
        // A perfectly horizontal line has no height to scale. Dividing by it
        // would put the whole path at infinity.
        let mut flat = kurbo::BezPath::new();
        flat.move_to((0.0, 0.0));
        flat.line_to((10.0, 0.0));
        let out = fit_to_bounds(
            &flat,
            DocRect {
                x: 5.0,
                y: 7.0,
                width: 20.0,
                height: 0.0,
            },
        );
        let b = out.bounding_box();
        assert!(b.y0.is_finite() && b.y0.abs() < 1e-9, "y0 {}", b.y0);
        assert!((b.width() - 20.0).abs() < 1e-9);
    }
}
