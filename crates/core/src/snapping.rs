//! Snapping during drag and resize.
//!
//! While a frame is being moved or resized, its edges and centre are compared
//! against nearby guides — page edges, margins, columns, ruler guides and other
//! frames' edges. When one falls within a threshold, the frame is nudged onto
//! it exactly.
//!
//! The threshold is expressed in *screen* pixels and divided by the zoom, so
//! snapping feels the same whether the user is zoomed to 25% or 400%. That is
//! the detail that makes snapping feel right rather than sticky or unreachable.

use serde::{Deserialize, Serialize};

use crate::components::BoundingBox;
use crate::layout::{PageGuides, PagePlacement};

/// Distance in screen pixels within which an edge snaps.
pub const DEFAULT_SNAP_THRESHOLD_PX: f32 = 5.0;

/// What a candidate snap line came from, so the UI can colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapSource {
    PageEdge,
    Margin,
    Column,
    /// A user-dragged ruler guide.
    Guide,
    /// An edge or centre of another frame.
    Object,
}

/// One line a frame can snap to, on a single axis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SnapLine {
    /// Position along the axis, in document units.
    pub position: f32,
    pub source: SnapSource,
}

/// Candidate snap lines, separated by axis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapTargets {
    pub vertical: Vec<SnapLine>,
    pub horizontal: Vec<SnapLine>,
}

impl SnapTargets {
    /// Collects the lines a page contributes: its edges, margins and columns.
    pub fn from_page(&mut self, page: &PagePlacement, guides: &PageGuides) {
        let (trim_x0, trim_y0, trim_x1, trim_y1) = page.trim_rect();
        for x in [trim_x0, trim_x1] {
            self.vertical.push(SnapLine { position: x, source: SnapSource::PageEdge });
        }
        for y in [trim_y0, trim_y1] {
            self.horizontal.push(SnapLine { position: y, source: SnapSource::PageEdge });
        }

        let (m_x0, m_y0, m_x1, m_y1) = guides.content_rect(page);
        for x in [m_x0, m_x1] {
            self.vertical.push(SnapLine { position: x, source: SnapSource::Margin });
        }
        for y in [m_y0, m_y1] {
            self.horizontal.push(SnapLine { position: y, source: SnapSource::Margin });
        }

        for (start, end) in guides.column_ranges(page) {
            self.vertical.push(SnapLine { position: start, source: SnapSource::Column });
            self.vertical.push(SnapLine { position: end, source: SnapSource::Column });
        }
    }

    /// Adds another frame's edges and centre as targets.
    ///
    /// Centres are included so objects can be aligned to each other, not just
    /// abutted — the alignment designers reach for most often.
    pub fn from_object(&mut self, bounds: &BoundingBox) {
        let (cx, cy) = bounds.center();
        for x in [bounds.min_x, cx, bounds.max_x] {
            self.vertical.push(SnapLine { position: x, source: SnapSource::Object });
        }
        for y in [bounds.min_y, cy, bounds.max_y] {
            self.horizontal.push(SnapLine { position: y, source: SnapSource::Object });
        }
    }

    /// Adds a user-placed ruler guide.
    pub fn from_ruler_guide(&mut self, guide: &RulerGuide) {
        let line = SnapLine { position: guide.position, source: SnapSource::Guide };
        match guide.axis {
            GuideAxis::Vertical => self.vertical.push(line),
            GuideAxis::Horizontal => self.horizontal.push(line),
        }
    }
}

/// Which way a ruler guide runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuideAxis {
    /// A vertical line at a fixed x.
    Vertical,
    /// A horizontal line at a fixed y.
    Horizontal,
}

/// A guide the user dragged off a ruler.
#[derive(bevy_ecs::prelude::Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RulerGuide {
    pub axis: GuideAxis,
    pub position: f32,
}

/// The outcome of snapping a moving frame.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct SnapResult {
    /// Correction to add to the frame's x, in document units.
    pub delta_x: f32,
    pub delta_y: f32,
    /// The line that was snapped to on each axis, for drawing feedback.
    pub snapped_vertical: Option<SnapLine>,
    pub snapped_horizontal: Option<SnapLine>,
}

impl SnapResult {
    pub fn is_snapped(&self) -> bool {
        self.snapped_vertical.is_some() || self.snapped_horizontal.is_some()
    }
}

/// Finds the correction that snaps `moving` onto the nearest targets.
///
/// The frame's leading edge, centre and trailing edge are all candidates on
/// each axis; whichever pairing is closest wins, provided it is inside the
/// threshold. `zoom` converts the screen-space threshold into document units.
pub fn snap_bounds(
    moving: &BoundingBox,
    targets: &SnapTargets,
    zoom: f32,
    threshold_px: f32,
) -> SnapResult {
    let tolerance = (threshold_px / zoom.max(f32::EPSILON)).abs();
    let (cx, cy) = moving.center();

    let (delta_x, snapped_vertical) =
        best_snap(&[moving.min_x, cx, moving.max_x], &targets.vertical, tolerance);
    let (delta_y, snapped_horizontal) =
        best_snap(&[moving.min_y, cy, moving.max_y], &targets.horizontal, tolerance);

    SnapResult {
        delta_x,
        delta_y,
        snapped_vertical,
        snapped_horizontal,
    }
}

/// The smallest correction bringing any of `edges` onto any of `lines`.
fn best_snap(edges: &[f32], lines: &[SnapLine], tolerance: f32) -> (f32, Option<SnapLine>) {
    let mut best: Option<(f32, SnapLine)> = None;

    for edge in edges {
        for line in lines {
            let delta = line.position - edge;
            if delta.abs() > tolerance {
                continue;
            }
            let closer = match best {
                // Ties favour the earlier candidate, which keeps the leading
                // edge winning over the centre when both are equidistant.
                Some((current, _)) => delta.abs() < current.abs(),
                None => true,
            };
            if closer {
                best = Some((delta, *line));
            }
        }
    }

    match best {
        Some((delta, line)) => (delta, Some(line)),
        None => (0.0, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::SpreadLayout;

    fn bounds(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> BoundingBox {
        BoundingBox::new(min_x, min_y, max_x, max_y)
    }

    fn vertical_targets(positions: &[f32]) -> SnapTargets {
        SnapTargets {
            vertical: positions
                .iter()
                .map(|p| SnapLine { position: *p, source: SnapSource::Guide })
                .collect(),
            horizontal: Vec::new(),
        }
    }

    #[test]
    fn an_edge_just_inside_the_threshold_snaps() {
        let targets = vertical_targets(&[100.0]);
        let result = snap_bounds(&bounds(97.0, 0.0, 197.0, 50.0), &targets, 1.0, 5.0);

        assert_eq!(result.delta_x, 3.0, "should pull the left edge onto 100");
        assert!(result.snapped_vertical.is_some());
    }

    #[test]
    fn an_edge_outside_the_threshold_does_not_snap() {
        let targets = vertical_targets(&[100.0]);
        let result = snap_bounds(&bounds(90.0, 0.0, 190.0, 50.0), &targets, 1.0, 5.0);

        assert_eq!(result.delta_x, 0.0);
        assert!(!result.is_snapped());
    }

    #[test]
    fn the_threshold_scales_with_zoom() {
        // The decisive property: at 4x zoom the same 5px threshold covers a
        // quarter of the document distance, so a snap that lands when zoomed
        // out must miss when zoomed in.
        let targets = vertical_targets(&[100.0]);
        let moving = bounds(96.0, 0.0, 196.0, 50.0);

        let zoomed_out = snap_bounds(&moving, &targets, 1.0, 5.0);
        let zoomed_in = snap_bounds(&moving, &targets, 4.0, 5.0);

        assert!(zoomed_out.is_snapped(), "4 units is within 5px at 100%");
        assert!(!zoomed_in.is_snapped(), "4 units exceeds 1.25 units at 400%");
    }

    #[test]
    fn the_nearest_target_wins() {
        let targets = vertical_targets(&[100.0, 104.0]);
        let result = snap_bounds(&bounds(103.0, 0.0, 200.0, 50.0), &targets, 1.0, 5.0);

        assert_eq!(result.delta_x, 1.0, "104 is nearer than 100");
    }

    #[test]
    fn the_trailing_edge_can_snap_too() {
        let targets = vertical_targets(&[200.0]);
        let result = snap_bounds(&bounds(98.0, 0.0, 198.0, 50.0), &targets, 1.0, 5.0);

        assert_eq!(result.delta_x, 2.0, "the right edge should reach 200");
    }

    #[test]
    fn centres_align_to_targets() {
        // Centre alignment is what lets a designer centre an object on a guide.
        let targets = vertical_targets(&[150.0]);
        let result = snap_bounds(&bounds(98.0, 0.0, 198.0, 50.0), &targets, 1.0, 5.0);

        assert_eq!(result.delta_x, 2.0, "centre 148 should reach 150");
    }

    #[test]
    fn both_axes_snap_independently() {
        let targets = SnapTargets {
            vertical: vec![SnapLine { position: 100.0, source: SnapSource::Margin }],
            horizontal: vec![SnapLine { position: 60.0, source: SnapSource::Margin }],
        };
        let result = snap_bounds(&bounds(97.0, 58.0, 197.0, 158.0), &targets, 1.0, 5.0);

        assert_eq!(result.delta_x, 3.0);
        assert_eq!(result.delta_y, 2.0);
        assert!(result.snapped_vertical.is_some() && result.snapped_horizontal.is_some());
    }

    #[test]
    fn no_targets_means_no_correction() {
        let result = snap_bounds(&bounds(0.0, 0.0, 10.0, 10.0), &SnapTargets::default(), 1.0, 5.0);

        assert_eq!((result.delta_x, result.delta_y), (0.0, 0.0));
        assert!(!result.is_snapped());
    }

    #[test]
    fn a_page_contributes_edges_margins_and_columns() {
        let layout = SpreadLayout {
            facing_pages: false,
            page_width: 100.0,
            page_height: 200.0,
            spread_gap: 0.0,
        };
        let guides = PageGuides {
            margin_top: 10.0,
            margin_bottom: 10.0,
            margin_inside: 10.0,
            margin_outside: 10.0,
            columns: 2,
            gutter: 10.0,
        };

        let mut targets = SnapTargets::default();
        targets.from_page(&layout.place(1), &guides);

        // Page edges at 0 and 100, margins at 10 and 90, plus column bounds.
        let verticals: Vec<f32> = targets.vertical.iter().map(|l| l.position).collect();
        assert!(verticals.contains(&0.0) && verticals.contains(&100.0));
        assert!(verticals.contains(&10.0) && verticals.contains(&90.0));
        assert!(targets
            .vertical
            .iter()
            .any(|l| l.source == SnapSource::Column));
    }

    #[test]
    fn objects_contribute_edges_and_centres() {
        let mut targets = SnapTargets::default();
        targets.from_object(&bounds(10.0, 20.0, 110.0, 120.0));

        let verticals: Vec<f32> = targets.vertical.iter().map(|l| l.position).collect();
        assert!(verticals.contains(&10.0));
        assert!(verticals.contains(&60.0), "centre should be a target");
        assert!(verticals.contains(&110.0));
    }

    #[test]
    fn ruler_guides_land_on_the_matching_axis() {
        let mut targets = SnapTargets::default();
        targets.from_ruler_guide(&RulerGuide { axis: GuideAxis::Vertical, position: 42.0 });
        targets.from_ruler_guide(&RulerGuide { axis: GuideAxis::Horizontal, position: 84.0 });

        assert_eq!(targets.vertical.len(), 1);
        assert_eq!(targets.horizontal.len(), 1);
        assert_eq!(targets.vertical[0].position, 42.0);
        assert_eq!(targets.horizontal[0].position, 84.0);
    }

    #[test]
    fn a_zero_zoom_does_not_divide_by_zero() {
        // Guards a degenerate camera state rather than producing infinities.
        let targets = vertical_targets(&[100.0]);
        let result = snap_bounds(&bounds(0.0, 0.0, 10.0, 10.0), &targets, 0.0, 5.0);

        assert!(result.delta_x.is_finite());
    }
}
