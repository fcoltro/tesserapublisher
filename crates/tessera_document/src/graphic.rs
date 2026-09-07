//! Graphic frames: containers with content of their own.
//!
//! **A graphic frame is not a shape**, and making that explicit is what
//! everything about placed images depends on. A rectangle has a fill; a
//! graphic frame has *contents*, which sit inside it under a transform of
//! their own and are clipped by it. The two are different things that happen
//! to be drawn in the same place, and the previous codebase conflating them is
//! why an image could never be moved inside its frame.
//!
//! The frame and its content each have a transform. The frame's says where the
//! frame is on the page; the content's says where the picture sits inside the
//! frame. Cropping is what you get when the second is larger than the first,
//! which is why cropping needs no separate model.

use serde::{Deserialize, Serialize};
use tessera_geometry::{DocRect, Transform};

use crate::ids::LinkId;

/// What a graphic frame is showing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    /// The asset on disk. **Linked, never embedded** — a layout that swallows
    /// its images is a layout nobody can re-supply artwork for.
    pub link: LinkId,
    /// Where the content sits inside the frame, in the frame's own space.
    ///
    /// Independent of the frame's transform, which is the whole point: moving
    /// the frame moves the picture with it, and moving the picture inside the
    /// frame leaves the frame alone.
    pub inner: Transform,
}

/// How content is sized to its frame.
///
/// An operation rather than stored state: what persists is the transform it
/// produced. Storing the mode as well would be a second description of the
/// same fact, and the two would disagree the moment somebody nudged the
/// picture by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Fill the frame exactly, ignoring the content's proportions.
    Stretch,
    /// The largest size that fits entirely inside the frame.
    Proportionally,
    /// The smallest size that covers the frame. Crops.
    FillProportionally,
    /// Leave the size alone and put it in the middle.
    Centre,
}

/// The transform that fits `natural` content into `frame` the chosen way.
///
/// `frame` and the result are both in the frame's own space. Content of no
/// size cannot be fitted to anything and is left where it is — a zero divisor
/// is not a special case worth inventing an answer for.
pub fn fit(frame: DocRect, natural: (f64, f64), how: Fit) -> Transform {
    let (width, height) = natural;
    if width <= 0.0 || height <= 0.0 {
        return Transform::IDENTITY;
    }

    let (sx, sy) = match how {
        Fit::Stretch => (frame.width / width, frame.height / height),
        Fit::Proportionally => {
            let s = (frame.width / width).min(frame.height / height);
            (s, s)
        }
        Fit::FillProportionally => {
            let s = (frame.width / width).max(frame.height / height);
            (s, s)
        }
        Fit::Centre => (1.0, 1.0),
    };

    // Centred on the frame whatever the scale, which is what every one of
    // these means: "fit" without "centre" would put the content in a corner
    // and leave the slack on two sides rather than four.
    let placed = (width * sx, height * sy);
    let dx = frame.x + (frame.width - placed.0) / 2.0;
    let dy = frame.y + (frame.height - placed.1) / 2.0;

    Transform::scale_about(sx, sy, tessera_geometry::DocPoint::ZERO)
        .then(Transform::translate(dx, dy))
}

/// The size a frame would have to be to hold `natural` content exactly.
///
/// The other direction: rather than sizing the picture to the box, size the
/// box to the picture. Keeps the frame's top-left, because that is the corner
/// a person placed.
pub fn frame_to_content(frame: DocRect, natural: (f64, f64)) -> DocRect {
    let (width, height) = natural;
    if width <= 0.0 || height <= 0.0 {
        return frame;
    }
    DocRect {
        x: frame.x,
        y: frame.y,
        width,
        height,
    }
}

/// The resolution artwork is actually reproduced at, in pixels per inch.
///
/// **Effective**, not natural: a 300ppi photograph scaled to twice its size is
/// a 150ppi photograph, and it is the effective figure a printer cares about.
/// Returns `None` for artwork drawn at no size, which has no resolution rather
/// than an infinite one.
pub fn effective_ppi(pixels: (u32, u32), drawn: (f64, f64)) -> Option<(f64, f64)> {
    if drawn.0 <= 0.0 || drawn.1 <= 0.0 {
        return None;
    }
    // 72 points to the inch.
    Some((
        f64::from(pixels.0) / (drawn.0 / 72.0),
        f64::from(pixels.1) / (drawn.1 / 72.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocPoint;

    fn frame() -> DocRect {
        DocRect {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 100.0,
        }
    }

    /// Where the content's own box lands once the transform is applied.
    fn placed(natural: (f64, f64), how: Fit) -> DocRect {
        let t = fit(frame(), natural, how);
        let a = t.apply(DocPoint::ZERO);
        let b = t.apply(DocPoint {
            x: natural.0,
            y: natural.1,
        });
        DocRect {
            x: a.x,
            y: a.y,
            width: b.x - a.x,
            height: b.y - a.y,
        }
    }

    // --- effective resolution ------------------------------------------------

    #[test]
    fn artwork_at_its_natural_size_reports_seventy_two_ppi() {
        // A point is a 72nd of an inch, so one pixel per point is 72ppi.
        let at = effective_ppi((100, 100), (100.0, 100.0)).expect("a resolution");
        assert!((at.0 - 72.0).abs() < 1e-9);
    }

    #[test]
    fn scaling_artwork_up_halves_its_resolution() {
        // **The number a printer cares about**: a 300ppi photograph at twice
        // its size is a 150ppi photograph.
        let small = effective_ppi((600, 600), (144.0, 144.0)).expect("a resolution");
        let large = effective_ppi((600, 600), (288.0, 288.0)).expect("a resolution");
        assert!((small.0 - 2.0 * large.0).abs() < 1e-9);
    }

    #[test]
    fn a_three_hundred_ppi_placement_reports_three_hundred() {
        // 300ppi means 300 pixels to the inch, and an inch is 72 points.
        let at = effective_ppi((300, 300), (72.0, 72.0)).expect("a resolution");
        assert!((at.0 - 300.0).abs() < 1e-9, "got {}", at.0);
    }

    #[test]
    fn a_stretched_placement_reports_two_different_resolutions() {
        // Stretching is not proportional, so the two axes really do differ and
        // a single figure would hide it.
        let at = effective_ppi((300, 300), (72.0, 144.0)).expect("a resolution");
        assert!((at.0 - 300.0).abs() < 1e-9);
        assert!((at.1 - 150.0).abs() < 1e-9);
    }

    #[test]
    fn artwork_drawn_at_no_size_has_no_resolution_rather_than_an_infinite_one() {
        assert!(effective_ppi((300, 300), (0.0, 72.0)).is_none());
        assert!(effective_ppi((300, 300), (72.0, 0.0)).is_none());
    }
    #[test]
    fn stretching_fills_the_frame_exactly() {
        let at = placed((50.0, 50.0), Fit::Stretch);
        assert!((at.width - 200.0).abs() < 1e-9);
        assert!((at.height - 100.0).abs() < 1e-9);
        assert!((at.x - 10.0).abs() < 1e-9);
        assert!((at.y - 20.0).abs() < 1e-9);
    }

    #[test]
    fn fitting_proportionally_stays_inside_the_frame() {
        // A square in a wide frame is limited by the height.
        let at = placed((50.0, 50.0), Fit::Proportionally);
        assert!((at.width - 100.0).abs() < 1e-9, "got {}", at.width);
        assert!((at.height - 100.0).abs() < 1e-9);
        assert!(at.width <= frame().width + 1e-9);
        assert!(at.height <= frame().height + 1e-9);
    }

    #[test]
    fn fitting_proportionally_keeps_the_proportions() {
        let at = placed((80.0, 40.0), Fit::Proportionally);
        assert!(
            ((at.width / at.height) - 2.0).abs() < 1e-9,
            "two to one, still: {at:?}"
        );
    }

    #[test]
    fn filling_proportionally_covers_the_frame_and_crops() {
        let at = placed((50.0, 50.0), Fit::FillProportionally);
        assert!(at.width >= frame().width - 1e-9);
        assert!(at.height >= frame().height - 1e-9);
        assert!(
            at.height > frame().height,
            "a square covering a wide frame hangs above and below, which is the crop"
        );
    }

    #[test]
    fn centring_leaves_the_size_alone() {
        let at = placed((50.0, 40.0), Fit::Centre);
        assert!((at.width - 50.0).abs() < 1e-9);
        assert!((at.height - 40.0).abs() < 1e-9);
    }

    #[test]
    fn every_fit_centres_what_it_places() {
        // "Fit" without "centre" would put the content in a corner and leave
        // the slack on two sides rather than four.
        for how in [
            Fit::Stretch,
            Fit::Proportionally,
            Fit::FillProportionally,
            Fit::Centre,
        ] {
            let at = placed((80.0, 40.0), how);
            let middle = (at.x + at.width / 2.0, at.y + at.height / 2.0);
            let want = (
                frame().x + frame().width / 2.0,
                frame().y + frame().height / 2.0,
            );
            assert!(
                (middle.0 - want.0).abs() < 1e-9 && (middle.1 - want.1).abs() < 1e-9,
                "{how:?} put it at {middle:?} rather than {want:?}"
            );
        }
    }

    #[test]
    fn content_with_no_size_is_left_where_it_is() {
        // A zero divisor is not a special case worth inventing an answer for.
        for natural in [(0.0, 50.0), (50.0, 0.0), (0.0, 0.0)] {
            assert!(fit(frame(), natural, Fit::Proportionally).is_identity());
        }
    }

    #[test]
    fn fitting_the_frame_to_its_content_keeps_the_corner_it_was_placed_at() {
        let sized = frame_to_content(frame(), (300.0, 150.0));
        assert_eq!(sized.x, frame().x);
        assert_eq!(sized.y, frame().y);
        assert_eq!(sized.width, 300.0);
        assert_eq!(sized.height, 150.0);
    }

    #[test]
    fn a_frame_cannot_be_fitted_to_content_of_no_size() {
        assert_eq!(frame_to_content(frame(), (0.0, 0.0)), frame());
    }
}
