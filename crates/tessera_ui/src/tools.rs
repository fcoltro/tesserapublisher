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
        }
    }

    pub fn icon(self) -> crate::icons::Icon {
        match self {
            Self::Select => crate::icons::Icon::Select,
            Self::DirectSelect => crate::icons::Icon::Crosshair,
            Self::Rectangle => crate::icons::Icon::Rectangle,
            Self::Ellipse => crate::icons::Icon::Ellipse,
            Self::Line => crate::icons::Icon::Line,
            Self::Pen => crate::icons::Icon::Pen,
            Self::Text => crate::icons::Icon::Text,
            Self::Graphic => crate::icons::Icon::TextFrame,
            Self::Polygon => crate::icons::Icon::Ellipse,
            Self::Scissors => crate::icons::Icon::NoFill,
            Self::Hand => crate::icons::Icon::Hand,
            Self::Zoom => crate::icons::Icon::ZoomIn,
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
        }
    }

    pub const ALL: [Self; 12] = [
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
    Anchor,
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
        target: FrameId,
        origin: DocRect,
        placement: tessera_geometry::Transform,
        leaves: Vec<crate::transform::Origin>,
    },
    /// Rotating about a pivot.
    Rotate {
        center: DocPoint,
        leaves: Vec<crate::transform::Origin>,
    },
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
