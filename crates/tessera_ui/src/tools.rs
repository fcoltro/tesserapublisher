//! The tool state machine.

use tessera_document::ids::FrameId;
use tessera_geometry::{DocPoint, DocRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    /// Picks the parts of a thing rather than the thing: an anchor point on a
    /// path, or one object inside a group.
    ///
    /// Separate from Select rather than a modifier on it, as every drawing tool
    /// has it. The two answer different questions — "which object" and "which
    /// part of it" — and a single tool that guessed between them from what the
    /// pointer happened to be over would guess wrong at the worst moment.
    DirectSelect,
    Rectangle,
    Ellipse,
    Line,
    Pen,
    Text,
    /// Draws a picture box: a container to place artwork into.
    Graphic,
    /// Drags out a regular polygon, or a star when its inset is above zero.
    Polygon,
    /// Cuts a path where it is clicked.
    Scissors,
    Hand,
    /// Click to zoom in, hold Alt to zoom out, drag to zoom to what was
    /// dragged around.
    Zoom,
    /// Pick an object's appearance up, and put it on others.
    Eyedropper,
}

/// What the eyedropper is carrying: an object's appearance, with its
/// corners, and — off a text frame — its type. Held until the tool is put
/// down; Alt-click picks up afresh.
#[derive(Debug, Clone, PartialEq)]
pub struct Sampled {
    pub format: tessera_document::object_style::ObjectFormat,
    pub corners: tessera_document::corners::Corners,
    pub text: Option<tessera_text::story::CharacterFormat>,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::DirectSelect => "Direct select",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Pen => "Pen",
            Self::Text => "Text",
            Self::Graphic => "Picture box",
            Self::Polygon => "Polygon",
            Self::Scissors => "Scissors",
            Self::Hand => "Hand",
            Self::Zoom => "Zoom",
            Self::Eyedropper => "Eyedropper",
        }
    }

    pub fn icon(self) -> crate::icons::Icon {
        match self {
            Self::Select => crate::icons::Icon::Select,
            Self::DirectSelect => crate::icons::Icon::DirectSelect,
            Self::Rectangle => crate::icons::Icon::Rectangle,
            Self::Ellipse => crate::icons::Icon::Ellipse,
            Self::Line => crate::icons::Icon::Line,
            Self::Pen => crate::icons::Icon::Pen,
            Self::Text => crate::icons::Icon::Text,
            Self::Graphic => crate::icons::Icon::PictureFrame,
            Self::Polygon => crate::icons::Icon::Polygon,
            Self::Scissors => crate::icons::Icon::Scissors,
            Self::Hand => crate::icons::Icon::Hand,
            Self::Zoom => crate::icons::Icon::ZoomIn,
            Self::Eyedropper => crate::icons::Icon::Pipette,
        }
    }

    /// Whether a single drag draws a whole frame.
    ///
    /// The pen is excluded: it builds a path across many clicks and finishes
    /// on its own terms.
    pub fn draws(self) -> bool {
        matches!(
            self,
            Self::Rectangle
                | Self::Ellipse
                | Self::Line
                | Self::Text
                | Self::Graphic
                | Self::Polygon
        )
    }

    /// The single-key shortcut. These follow InDesign's, which is what a
    /// layout designer's fingers already know.
    pub fn shortcut(self) -> egui::Key {
        match self {
            Self::Select => egui::Key::V,
            // A, as InDesign's direct selection tool is.
            Self::DirectSelect => egui::Key::A,
            Self::Rectangle => egui::Key::M,
            Self::Ellipse => egui::Key::L,
            Self::Line => egui::Key::Backslash,
            Self::Pen => egui::Key::P,
            Self::Text => egui::Key::T,
            // F, as InDesign's frame tool is.
            Self::Graphic => egui::Key::F,
            Self::Hand => egui::Key::H,
            // G and C, as InDesign has them.
            Self::Polygon => egui::Key::G,
            Self::Scissors => egui::Key::C,
            Self::Zoom => egui::Key::Z,
            // I, as InDesign's eyedropper is.
            Self::Eyedropper => egui::Key::I,
        }
    }

    pub const ALL: [Self; 13] = [
        Self::Select,
        Self::DirectSelect,
        Self::Rectangle,
        Self::Ellipse,
        Self::Line,
        Self::Pen,
        Self::Text,
        Self::Graphic,
        Self::Polygon,
        Self::Scissors,
        Self::Eyedropper,
        Self::Hand,
        Self::Zoom,
    ];
}

/// What a drag in progress is doing.
#[derive(Debug, Clone, PartialEq)]
pub enum DragKind {
    /// Drawing a new frame.
    Draw,
    /// Moving one anchor point of a path.
    ///
    /// Carries the path and box the drag began from, so every step is
    /// measured from the origin and the whole drag is one undo entry.
    Anchor { held: crate::view::anchors::Held },
    /// Rubber-band selection over empty canvas.
    Marquee,
    /// Moving the selection.
    ///
    /// Carries each frame's **placement** at the moment the drag began, so the
    /// move is computed from the origin rather than accumulated per frame —
    /// which would drift, and would make a single undo entry impossible.
    ///
    /// The placement, not the bounds: `bounds` is expressed in the frame's own
    /// space, and a pointer delta is in document space. Adding one to the
    /// other turns the move by the frame's own angle, which sent a rotated
    /// frame off sideways and a half-turned one backwards.
    Move {
        origins: Vec<(FrameId, tessera_geometry::Transform)>,
    },
    /// Resizing by a handle.
    ///
    /// Carries the box the gesture started from and the starting state of
    /// every frame inside it — a group's children as well as the frame
    /// itself. Every step recomputes from these rather than from the previous
    /// step, so rounding cannot compound into drift and the whole gesture
    /// commits as one undo entry.
    Scale {
        handle: crate::transform::Handle,
        /// The frame the handle belongs to. It takes the new box directly;
        /// anything inside it follows by transform.
        ///
        /// `None` for a multiple selection, whose box is the upright one drawn
        /// around the whole of it. That box is nobody's own, so no frame may
        /// take it: every one of them follows by transform instead.
        target: Option<FrameId>,
        origin: DocRect,
        placement: tessera_geometry::Transform,
        leaves: Vec<crate::transform::Origin>,
    },
    /// Rotating about a pivot.
    Rotate {
        center: DocPoint,
        leaves: Vec<crate::transform::Origin>,
    },
    /// Dragging a page's right or bottom edge, or the corner where they
    /// meet, to make the page another size. Carries the size the page had
    /// when the drag began, so the new size is computed from it rather than
    /// accumulated.
    PageEdge {
        page: tessera_document::ids::PageId,
        edge: PageEdge,
        width: f64,
        height: f64,
    },
}

/// Which edge of a page a drag is pulling. The left and top stay: a page's
/// origin is where its spread put it, and the size is what changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageEdge {
    Right,
    Bottom,
    Corner,
}

impl PageEdge {
    /// The size a page of `width` by `height` becomes when this edge is
    /// dragged by `(dx, dy)`: never smaller than a postage stamp.
    pub fn resized(self, width: f64, height: f64, dx: f64, dy: f64) -> (f64, f64) {
        const SMALLEST: f64 = 36.0;
        let w = match self {
            PageEdge::Right | PageEdge::Corner => (width + dx).max(SMALLEST),
            PageEdge::Bottom => width,
        };
        let h = match self {
            PageEdge::Bottom | PageEdge::Corner => (height + dy).max(SMALLEST),
            PageEdge::Right => height,
        };
        (w, h)
    }
}

/// A gesture in progress.
///
/// Held in application state rather than in a widget, because an
/// immediate-mode widget does not survive between frames.
#[derive(Debug, Clone)]
pub struct Drag {
    pub start: DocPoint,
    pub current: DocPoint,
    pub kind: DragKind,
}

impl Drag {
    pub fn new(start: DocPoint, kind: DragKind) -> Self {
        Self {
            start,
            current: start,
            kind,
        }
    }

    /// The normalised rectangle the gesture describes, so dragging up-left
    /// produces the same rectangle as dragging down-right.
    pub fn rect(&self) -> DocRect {
        DocRect {
            x: self.start.x.min(self.current.x),
            y: self.start.y.min(self.current.y),
            width: (self.current.x - self.start.x).abs(),
            height: (self.current.y - self.start.y).abs(),
        }
    }

    pub fn delta(&self) -> (f64, f64) {
        (self.current.x - self.start.x, self.current.y - self.start.y)
    }

    /// The shape this drag would make with `tool`, in document space.
    ///
    /// **The preview is the shape, not its box.** A box says where an ellipse
    /// will land and nothing about what it will look like, and for a line it
    /// is the wrong diagonal half the time. The polygon is the very path the
    /// commit makes, so the two cannot drift apart; the ellipse is kurbo's, as
    /// the renderer draws it.
    pub fn preview(&self, tool: Tool, sides: u32, inset: f64) -> kurbo::BezPath {
        use kurbo::Shape as _;
        let r = self.rect();
        let bounds = kurbo::Rect::new(r.x, r.y, r.x + r.width, r.y + r.height);
        match tool {
            Tool::Ellipse => kurbo::Ellipse::from_rect(bounds).to_path(0.1),
            Tool::Line => {
                let mut path = kurbo::BezPath::new();
                path.move_to((self.start.x, self.start.y));
                path.line_to((self.current.x, self.current.y));
                path
            }
            Tool::Polygon => {
                let mut path = tessera_document::polygon::path(
                    DocRect {
                        x: 0.0,
                        y: 0.0,
                        width: r.width,
                        height: r.height,
                    },
                    sides,
                    inset,
                );
                path.apply_affine(kurbo::Affine::translate((r.x, r.y)));
                path
            }
            _ => bounds.to_path(0.1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_down_right_yields_the_expected_rectangle() {
        let mut d = Drag::new(DocPoint { x: 10.0, y: 20.0 }, DragKind::Draw);
        d.current = DocPoint { x: 40.0, y: 60.0 };
        assert_eq!(
            d.rect(),
            DocRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0
            }
        );
    }

    #[test]
    fn dragging_up_left_yields_the_same_rectangle() {
        let mut d = Drag::new(DocPoint { x: 40.0, y: 60.0 }, DragKind::Draw);
        d.current = DocPoint { x: 10.0, y: 20.0 };
        assert_eq!(
            d.rect(),
            DocRect {
                x: 10.0,
                y: 20.0,
                width: 30.0,
                height: 40.0
            }
        );
    }

    fn drawn(tool: Tool, sides: u32, inset: f64) -> kurbo::BezPath {
        let mut d = Drag::new(DocPoint { x: 10.0, y: 20.0 }, DragKind::Draw);
        d.current = DocPoint { x: 70.0, y: 60.0 };
        d.preview(tool, sides, inset)
    }

    fn corners(path: &kurbo::BezPath) -> Vec<kurbo::Point> {
        path.elements()
            .iter()
            .filter_map(|el| match el {
                kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => Some(*p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_polygon_previews_as_the_polygon_it_will_be() {
        // It previewed as its bounding box, which said where a hexagon would
        // land and nothing about its shape. Now the preview is the very path
        // the commit makes, placed where the drag is.
        let preview = drawn(Tool::Polygon, 6, 0.0);
        let committed = tessera_document::polygon::path(
            DocRect {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 40.0,
            },
            6,
            0.0,
        );
        let expected: Vec<kurbo::Point> = corners(&committed)
            .into_iter()
            .map(|p| kurbo::Point::new(p.x + 10.0, p.y + 20.0))
            .collect();
        assert_eq!(corners(&preview), expected);
    }

    #[test]
    fn a_line_previews_from_where_it_started_to_where_it_is() {
        // Not from the box's corner: a line dragged bottom-left to top-right
        // is the other diagonal of the same box.
        let mut d = Drag::new(DocPoint { x: 70.0, y: 60.0 }, DragKind::Draw);
        d.current = DocPoint { x: 10.0, y: 20.0 };
        assert_eq!(
            corners(&d.preview(Tool::Line, 6, 0.0)),
            vec![kurbo::Point::new(70.0, 60.0), kurbo::Point::new(10.0, 20.0)]
        );
    }

    #[test]
    fn an_ellipse_previews_as_a_curve_inside_its_box() {
        let path = drawn(Tool::Ellipse, 6, 0.0);
        assert!(
            path.elements()
                .iter()
                .any(|el| matches!(el, kurbo::PathEl::CurveTo(..))),
            "an ellipse is curves, not a box"
        );
        let bbox = kurbo::Shape::bounding_box(&path);
        assert!((bbox.x0 - 10.0).abs() < 1e-6 && (bbox.x1 - 70.0).abs() < 1e-6);
        assert!((bbox.y0 - 20.0).abs() < 1e-6 && (bbox.y1 - 60.0).abs() < 1e-6);
    }

    #[test]
    fn everything_else_previews_as_its_box() {
        for tool in [Tool::Rectangle, Tool::Text, Tool::Graphic] {
            let path = drawn(tool, 6, 0.0);
            let bbox = kurbo::Shape::bounding_box(&path);
            assert_eq!(bbox, kurbo::Rect::new(10.0, 20.0, 70.0, 60.0), "{tool:?}");
        }
    }

    #[test]
    fn a_drag_that_has_not_moved_has_no_area() {
        let d = Drag::new(DocPoint { x: 5.0, y: 5.0 }, DragKind::Draw);
        assert_eq!(d.rect().width, 0.0);
        assert_eq!(d.rect().height, 0.0);
    }

    #[test]
    fn delta_is_signed_even_though_the_rectangle_is_not() {
        // A move needs direction; a drawn frame does not. Both read the same
        // drag, so the two must not be conflated.
        let mut d = Drag::new(DocPoint { x: 40.0, y: 60.0 }, DragKind::Draw);
        d.current = DocPoint { x: 10.0, y: 20.0 };
        assert_eq!(d.delta(), (-30.0, -40.0));
        assert_eq!(d.rect().width, 30.0);
    }

    #[test]
    fn every_tool_has_a_distinct_shortcut() {
        let keys: Vec<_> = Tool::ALL.iter().map(|t| t.shortcut()).collect();
        let mut unique = keys.clone();
        unique.sort_by_key(|k| format!("{k:?}"));
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
    }

    #[test]
    fn only_the_drag_to_draw_tools_report_that_they_draw() {
        assert!(Tool::Rectangle.draws());
        assert!(Tool::Ellipse.draws());
        assert!(Tool::Line.draws());
        assert!(Tool::Text.draws());
        assert!(!Tool::Pen.draws(), "the pen builds a path across clicks");
        assert!(!Tool::Select.draws());
        assert!(!Tool::Hand.draws());
    }
}
