//! The tool state machine.

use tessera_geometry::{DocPoint, DocRect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Rectangle,
    Ellipse,
    Line,
    Text,
    Hand,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Text => "Text",
            Self::Hand => "Hand",
        }
    }

    pub fn icon(self) -> crate::icons::Icon {
        match self {
            Self::Select => crate::icons::Icon::Select,
            Self::Rectangle => crate::icons::Icon::Rectangle,
            Self::Ellipse => crate::icons::Icon::Ellipse,
            Self::Line => crate::icons::Icon::Line,
            Self::Text => crate::icons::Icon::Text,
            Self::Hand => crate::icons::Icon::Hand,
        }
    }

    /// Whether dragging this tool draws a new frame.
    pub fn draws(self) -> bool {
        matches!(
            self,
            Self::Rectangle | Self::Ellipse | Self::Line | Self::Text
        )
    }

    /// The single-key shortcut. These follow InDesign's, which is what a
    /// layout designer's fingers already know.
    pub fn shortcut(self) -> egui::Key {
        match self {
            Self::Select => egui::Key::V,
            Self::Rectangle => egui::Key::M,
            Self::Ellipse => egui::Key::L,
            Self::Line => egui::Key::Backslash,
            Self::Text => egui::Key::T,
            Self::Hand => egui::Key::H,
        }
    }

    pub const ALL: [Self; 6] = [
        Self::Select,
        Self::Rectangle,
        Self::Ellipse,
        Self::Line,
        Self::Text,
        Self::Hand,
    ];
}

/// A gesture in progress. Held in application state rather than in a widget,
/// because an immediate-mode widget does not survive between frames.
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub start: DocPoint,
    pub current: DocPoint,
    /// For a move gesture: the frame's bounds when the drag began, so the move
    /// is always computed from the origin rather than accumulated per frame.
    pub origin_bounds: Option<DocRect>,
}

impl Drag {
    pub fn new(start: DocPoint) -> Self {
        Self {
            start,
            current: start,
            origin_bounds: None,
        }
    }

    /// The normalized rectangle the gesture describes, so dragging up-left
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
        let mut d = Drag::new(DocPoint { x: 10.0, y: 20.0 });
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
        let mut d = Drag::new(DocPoint { x: 40.0, y: 60.0 });
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
        let d = Drag::new(DocPoint { x: 5.0, y: 5.0 });
        assert_eq!(d.rect().width, 0.0);
        assert_eq!(d.rect().height, 0.0);
    }

    #[test]
    fn every_tool_has_a_distinct_shortcut() {
        let keys: Vec<_> = Tool::ALL.iter().map(|t| t.shortcut()).collect();
        let mut unique = keys.clone();
        unique.sort_by_key(|k| format!("{k:?}"));
        unique.dedup();
        assert_eq!(unique.len(), keys.len());
    }
}
