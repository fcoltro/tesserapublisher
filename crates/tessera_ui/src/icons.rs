//! Icons, as geometry rather than assets.
//!
//! The shapes are [Lucide](https://lucide.dev) — drawn on a 24×24 grid with a
//! 2px round-capped stroke — stored here as SVG path data, parsed by `kurbo`
//! (already a dependency), and painted through `egui::Painter`.
//!
//! No image files, no SVG renderer, no icon font. The icons stay crisp at any
//! DPI, re-tint with [`crate::theme`], and add nothing to the binary but a few
//! hundred bytes of text.
//!
//! Lucide is ISC-licensed; icons inherited from Feather are MIT. See
//! `ATTRIBUTION.md`.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use kurbo::{BezPath, PathEl};

/// The grid Lucide draws on.
const GRID: f32 = 24.0;
/// Lucide's stroke width, in grid units.
const STROKE: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Select,
    Rectangle,
    Ellipse,
    Line,
    Pen,
    Text,
    Hand,
}

impl Icon {
    /// SVG path data, in the 24×24 Lucide grid.
    ///
    /// Lucide's `<rect>` and `<circle>` primitives are written out as paths
    /// here so that everything goes through one parser.
    pub fn paths(self) -> &'static [&'static str] {
        match self {
            // lucide: mouse-pointer-2
            Self::Select => &[
                "M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z",
            ],
            // lucide: square — <rect width=18 height=18 x=3 y=3 rx=2>
            Self::Rectangle => &[
                "M5 3 h14 a2 2 0 0 1 2 2 v14 a2 2 0 0 1 -2 2 h-14 a2 2 0 0 1 -2 -2 v-14 a2 2 0 0 1 2 -2 z",
            ],
            // lucide: circle — <circle cx=12 cy=12 r=10>
            Self::Ellipse => &["M22 12 A10 10 0 1 1 2 12 A10 10 0 1 1 22 12 Z"],
            // lucide: slash
            Self::Line => &["M22 2 2 22"],
            // lucide: pen-tool
            Self::Pen => &[
                "M15.707 21.293a1 1 0 0 1-1.414 0l-1.586-1.586a1 1 0 0 1 0-1.414l5.586-5.586a1 1 0 0 1 1.414 0l1.586 1.586a1 1 0 0 1 0 1.414z",
                "m18 13-1.375-6.874a1 1 0 0 0-.746-.776L3.235 2.028a1 1 0 0 0-1.207 1.207L5.35 15.879a1 1 0 0 0 .776.746L13 18",
                "m2.3 2.3 7.286 7.286",
                "M13 11 A2 2 0 1 1 9 11 A2 2 0 1 1 13 11 Z",
            ],
            // lucide: type
            Self::Text => &[
                "M12 4v16",
                "M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2",
                "M9 20h6",
            ],
            // lucide: hand
            Self::Hand => &[
                "M18 11V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2",
                "M14 10V4a2 2 0 0 0-2-2a2 2 0 0 0-2 2v2",
                "M10 10.5V6a2 2 0 0 0-2-2a2 2 0 0 0-2 2v8",
                "M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15",
            ],
        }
    }
}

/// Paint `icon` to fill `rect`, stroked in `color`.
///
/// The icon is scaled uniformly from its 24-unit grid, so the stroke stays
/// proportional and the shape never distorts.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let side = rect.width().min(rect.height());
    let scale = side / GRID;
    let origin = rect.center() - egui::vec2(side / 2.0, side / 2.0);
    let stroke = Stroke::new(STROKE * scale, color);

    // Flatten in grid units, then scale — so the tolerance means the same
    // thing regardless of how large the icon is drawn.
    let tolerance = 0.05 / f64::from(scale.max(f32::EPSILON));

    for data in icon.paths() {
        let Ok(path) = BezPath::from_svg(data) else {
            // Unreachable: `every_icon_parses` pins this at test time. Drawing
            // nothing is still better than panicking in a paint loop.
            debug_assert!(false, "icon path failed to parse: {data}");
            continue;
        };

        let mut run: Vec<Pos2> = Vec::new();
        let flush = |run: &mut Vec<Pos2>| {
            if run.len() > 1 {
                painter.add(Shape::line(std::mem::take(run), stroke));
            } else {
                run.clear();
            }
        };

        kurbo::flatten(path.iter(), tolerance, |el| {
            let at = |p: kurbo::Point| origin + egui::vec2(p.x as f32 * scale, p.y as f32 * scale);
            match el {
                PathEl::MoveTo(p) => {
                    flush(&mut run);
                    run.push(at(p));
                }
                PathEl::LineTo(p) => run.push(at(p)),
                PathEl::ClosePath => {
                    if let Some(first) = run.first().copied() {
                        run.push(first);
                    }
                    flush(&mut run);
                }
                // `flatten` emits only MoveTo, LineTo and ClosePath.
                PathEl::QuadTo(..) | PathEl::CurveTo(..) => {}
            }
        });
        flush(&mut run);
    }
}

/// Every icon, for exhaustive tests and for building a palette.
pub const ALL: [Icon; 7] = [
    Icon::Select,
    Icon::Rectangle,
    Icon::Ellipse,
    Icon::Line,
    Icon::Pen,
    Icon::Text,
    Icon::Hand,
];

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape as _;

    #[test]
    fn every_icon_parses() {
        // `paint` relies on this, so it must be checked rather than assumed.
        for icon in ALL {
            for data in icon.paths() {
                assert!(
                    BezPath::from_svg(data).is_ok(),
                    "{icon:?} has an unparseable path: {data}"
                );
            }
        }
    }

    #[test]
    fn every_icon_produces_real_geometry() {
        for icon in ALL {
            let segments: usize = icon
                .paths()
                .iter()
                .map(|d| BezPath::from_svg(d).expect("parses").segments().count())
                .sum();
            assert!(segments > 0, "{icon:?} draws nothing");
        }
    }

    #[test]
    fn every_icon_stays_inside_the_lucide_grid() {
        // A path outside 0..24 would be clipped or mis-scaled when painted.
        for icon in ALL {
            for data in icon.paths() {
                let b = BezPath::from_svg(data).expect("parses").bounding_box();
                assert!(
                    b.x0 >= -0.5 && b.y0 >= -0.5 && b.x1 <= 24.5 && b.y1 <= 24.5,
                    "{icon:?} escapes the 24x24 grid: {b:?}"
                );
            }
        }
    }

    #[test]
    fn the_arc_based_icons_really_close_into_a_ring() {
        // The circle and the pen's nib are written as SVG arcs. If kurbo's arc
        // handling were wrong they would parse but draw an open sliver, so
        // check the ellipse spans the full grid in both axes.
        let b = BezPath::from_svg(Icon::Ellipse.paths()[0])
            .expect("parses")
            .bounding_box();
        assert!((b.width() - 20.0).abs() < 0.5, "width was {}", b.width());
        assert!((b.height() - 20.0).abs() < 0.5, "height was {}", b.height());
    }
}
