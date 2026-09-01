//! Mapping between document space and screen space.

use serde::{Deserialize, Serialize};

use crate::spaces::{DocPoint, DocRect, ScreenPoint};

/// Maps document space to screen space.
///
/// Owned by the viewport, never by the document — panning is not an edit, and
/// must not mark a document dirty or land in the undo stack.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewTransform {
    pub pan: DocPoint,
    pub zoom: f64,
}

impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            pan: DocPoint::ZERO,
            zoom: 1.0,
        }
    }
}

impl ViewTransform {
    pub fn doc_to_screen(&self, p: DocPoint) -> ScreenPoint {
        ScreenPoint {
            x: ((p.x - self.pan.x) * self.zoom) as f32,
            y: ((p.y - self.pan.y) * self.zoom) as f32,
        }
    }

    pub fn screen_to_doc(&self, p: ScreenPoint) -> DocPoint {
        DocPoint {
            x: f64::from(p.x) / self.zoom + self.pan.x,
            y: f64::from(p.y) / self.zoom + self.pan.y,
        }
    }

    pub fn doc_rect_to_screen(&self, r: DocRect) -> (ScreenPoint, ScreenPoint) {
        (
            self.doc_to_screen(DocPoint { x: r.x, y: r.y }),
            self.doc_to_screen(DocPoint {
                x: r.x + r.width,
                y: r.y + r.height,
            }),
        )
    }

    /// The equivalent affine, for handing to Vello.
    pub fn to_affine(&self) -> kurbo::Affine {
        kurbo::Affine::scale(self.zoom) * kurbo::Affine::translate((-self.pan.x, -self.pan.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spaces::{DocPoint, ScreenPoint};

    #[test]
    fn screen_to_doc_inverts_doc_to_screen() {
        let view = ViewTransform {
            pan: DocPoint { x: 100.0, y: 50.0 },
            zoom: 2.5,
        };
        let original = DocPoint { x: 42.0, y: -17.5 };

        let round_tripped = view.screen_to_doc(view.doc_to_screen(original));

        assert!((round_tripped.x - original.x).abs() < 1e-6);
        assert!((round_tripped.y - original.y).abs() < 1e-6);
    }

    #[test]
    fn zoom_scales_distance_from_the_pan_origin() {
        let view = ViewTransform {
            pan: DocPoint::ZERO,
            zoom: 2.0,
        };
        let screen = view.doc_to_screen(DocPoint { x: 10.0, y: 10.0 });
        assert_eq!(screen, ScreenPoint { x: 20.0, y: 20.0 });
    }

    #[test]
    fn panning_shifts_the_origin() {
        let view = ViewTransform {
            pan: DocPoint { x: 10.0, y: 0.0 },
            zoom: 1.0,
        };
        assert_eq!(
            view.doc_to_screen(DocPoint { x: 10.0, y: 0.0 }),
            ScreenPoint { x: 0.0, y: 0.0 }
        );
    }

    proptest::proptest! {
        #[test]
        fn round_trip_holds_for_any_view(
            px in -10_000.0f64..10_000.0,
            py in -10_000.0f64..10_000.0,
            zoom in 0.01f64..64.0,
            x in -10_000.0f64..10_000.0,
            y in -10_000.0f64..10_000.0,
        ) {
            let view = ViewTransform { pan: DocPoint { x: px, y: py }, zoom };
            let original = DocPoint { x, y };
            let back = view.screen_to_doc(view.doc_to_screen(original));
            // f32 screen coordinates bound the achievable precision.
            proptest::prop_assert!((back.x - original.x).abs() < 0.01);
            proptest::prop_assert!((back.y - original.y).abs() < 0.01);
        }
    }
}
