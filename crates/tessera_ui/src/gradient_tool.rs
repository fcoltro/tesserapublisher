//! InDesign's Gradient Swatch tool and Gradient Feather tool: drag across an
//! object to say where its gradient — of colour, or of opacity — starts and
//! where it ends.
//!
//! The drag becomes a [`Span`]: two points in fractions of the object's own
//! box, so the gradient goes with the object when it moves and stretches
//! when it is resized. A linear ramp runs between them, and its angle is set
//! to the drag's so the Properties panel reads what was drawn; a radial ramp
//! is centred on the first and reaches the second.
//!
//! Applied to the object under the press, or the one selected when the
//! press misses everything; one undo step either way.

use tessera_document::ids::FrameId;
use tessera_document::paint::{Gradient, Paint, Ramp, Span};
use tessera_geometry::DocPoint;

use crate::app::TesseraApp;
use crate::command::{Command, apply};

/// Which of the two tools made the drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The fill's colours.
    Swatch,
    /// The object's opacity.
    Feather,
}

/// The span a drag from `from` to `to`, in the document, makes on `id`.
pub fn span_of(state: &TesseraApp, id: FrameId, from: DocPoint, to: DocPoint) -> Option<Span> {
    let frame = state.active().document().frame(id)?;
    let back = frame.transform.inverse();
    Some(Span::between(
        back.apply(from),
        back.apply(to),
        frame.bounds,
    ))
}

/// Lay the dragged gradient onto `id`.
pub fn apply_drag(state: &mut TesseraApp, kind: Kind, id: FrameId, from: DocPoint, to: DocPoint) {
    let Some(span) = span_of(state, id, from, to) else {
        return;
    };
    let Some(frame) = state.active().document().frame(id).cloned() else {
        return;
    };
    let angle = span.angle(frame.bounds);
    let aimed = |ramp: Ramp| match ramp {
        Ramp::Linear { .. } => Ramp::Linear { angle },
        Ramp::Radial => Ramp::Radial,
    };
    match kind {
        Kind::Swatch => {
            // A solid becomes InDesign's first gradient, black to white; a
            // gradient keeps its colours and takes the new direction.
            let mut gradient = match &frame.fill {
                Paint::Gradient(g) => g.clone(),
                Paint::Solid(_) => Gradient::black_to_white(Ramp::Linear { angle }),
            };
            gradient.ramp = aimed(gradient.ramp);
            gradient.span = Some(span);
            apply(
                state,
                Command::SetFill {
                    id,
                    paint: Paint::Gradient(gradient),
                },
            );
        }
        Kind::Feather => {
            let mut feather = frame.feather.clone().unwrap_or_default();
            feather.ramp = aimed(feather.ramp);
            feather.span = Some(span);
            apply(
                state,
                Command::SetFeather {
                    id,
                    feather: Some(feather),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::feather::GradientFeather;
    use tessera_geometry::DocRect;

    fn a_rectangle() -> (TesseraApp, FrameId, DocRect) {
        let mut state = TesseraApp::headless();
        let page = state.first_page_bounds();
        let bounds = DocRect {
            x: page.x + 20.0,
            y: page.y + 20.0,
            width: 100.0,
            height: 50.0,
        };
        apply(&mut state, Command::AddRectangle(bounds));
        let id = state.active().selection.single().expect("drawn");
        (state, id, bounds)
    }

    #[test]
    fn a_drag_makes_a_gradient_run_where_it_went_in_one_undo() {
        let (mut state, id, b) = a_rectangle();
        let from = DocPoint {
            x: b.x + 25.0,
            y: b.y + 25.0,
        };
        let to = DocPoint {
            x: b.x + 75.0,
            y: b.y + 25.0,
        };
        apply_drag(&mut state, Kind::Swatch, id, from, to);
        let Paint::Gradient(g) = &state.active().document().frame(id).unwrap().fill else {
            panic!("a gradient now");
        };
        assert_eq!(
            g.span,
            Some(Span {
                from: (0.25, 0.5),
                to: (0.75, 0.5)
            })
        );
        assert_eq!(g.ramp, Ramp::Linear { angle: 0.0 });
        let (a, z) = g.axis(b);
        assert!((a.x - from.x).abs() < 1e-9 && (z.x - to.x).abs() < 1e-9);
        apply(&mut state, Command::Undo);
        assert!(matches!(
            state.active().document().frame(id).unwrap().fill,
            Paint::Solid(_)
        ));
    }

    #[test]
    fn a_feather_drag_aims_the_fade_and_keeps_its_stops() {
        let (mut state, id, b) = a_rectangle();
        let from = DocPoint { x: b.x, y: b.y };
        let to = DocPoint {
            x: b.x,
            y: b.y + 50.0,
        };
        apply_drag(&mut state, Kind::Feather, id, from, to);
        let feather = state
            .active()
            .document()
            .frame(id)
            .unwrap()
            .feather
            .clone()
            .expect("on");
        assert_eq!(feather.ramp, Ramp::Linear { angle: 90.0 });
        assert_eq!(
            feather.span,
            Some(Span {
                from: (0.0, 0.0),
                to: (0.0, 1.0)
            })
        );
        assert_eq!(feather.stops(), GradientFeather::default().stops());
    }
}
