//! Resolves a document into drawable items.
//!
//! Both the screen renderer and the PDF writer consume the output of this
//! module, so neither re-derives geometry nor re-shapes text. That shared
//! source is what keeps an export from drifting away from the screen.

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::ids::FrameId;
use tessera_document::nodes::{FrameKind, Stroke};
use tessera_geometry::DocRect;
use tessera_text::shape::{ShapedText, Shaper};

pub use tessera_document::document::StoryMap;

#[derive(Debug, Clone)]
pub enum ResolvedKind {
    Rectangle {
        fill: Color,
        stroke: Option<Stroke>,
    },
    Ellipse {
        fill: Color,
        stroke: Option<Stroke>,
    },
    Text {
        shaped: ShapedText,
        color: Color,
    },
    /// A path in frame-local coordinates. Consumers translate by
    /// [`ResolvedItem::bounds`]'s origin.
    Path {
        path: kurbo::BezPath,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedItem {
    pub frame: FrameId,
    pub bounds: DocRect,
    /// Clockwise degrees about `bounds.center()`. Both the renderer and the
    /// PDF writer apply this the same way, from this one value.
    pub rotation: f64,
    pub kind: ResolvedKind,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedDocument {
    /// Back to front. The last item paints on top.
    pub items: Vec<ResolvedItem>,
}

/// Resolve every visible frame, in paint order.
pub fn resolve(doc: &Document, shaper: &mut Shaper) -> ResolvedDocument {
    let mut items = Vec::new();

    for id in doc.paint_order() {
        let Some(frame) = doc.frame(id) else { continue };

        let kind = match &frame.kind {
            FrameKind::Rectangle => ResolvedKind::Rectangle {
                fill: frame.fill.clone(),
                stroke: frame.stroke.clone(),
            },
            FrameKind::Ellipse => ResolvedKind::Ellipse {
                fill: frame.fill.clone(),
                stroke: frame.stroke.clone(),
            },
            FrameKind::Path(path) => ResolvedKind::Path {
                path: path.clone(),
                // An open path with no explicit stroke would be invisible, so
                // a path frame's fill is treated as its stroke colour when it
                // has no stroke of its own.
                fill: None,
                stroke: Some(frame.stroke.clone().unwrap_or(Stroke {
                    color: frame.fill.clone(),
                    width: 1.0,
                })),
            },

            FrameKind::Text { story } => {
                // A text frame whose story is missing is a broken document,
                // not a blank frame. Skipping it silently would hide the
                // breakage; milestone 6's preflight reports it. For now it
                // simply does not paint, which is visible.
                let Some(story) = doc.story(*story) else {
                    continue;
                };
                ResolvedKind::Text {
                    shaped: shaper.shape(story, frame.bounds.width),
                    color: story.style.color.clone(),
                }
            }
        };

        items.push(ResolvedItem {
            frame: id,
            bounds: frame.bounds,
            rotation: frame.rotation,
            kind,
        });
    }

    ResolvedDocument { items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::Frame;
    use tessera_text::story::Story;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Frame {
        Frame {
            bounds: DocRect {
                x,
                y,
                width: w,
                height: h,
            },
            kind: FrameKind::Rectangle,
            rotation: 0.0,
            fill: Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn an_empty_document_resolves_to_nothing() {
        let resolved = resolve(&Document::new(), &mut Shaper::new());
        assert!(resolved.items.is_empty());
    }

    #[test]
    fn a_rectangle_resolves_to_a_rectangle_at_its_bounds() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(layer, rect(5.0, 6.0, 20.0, 30.0));

        let resolved = resolve(&doc, &mut Shaper::new());

        assert_eq!(resolved.items.len(), 1);
        assert_eq!(resolved.items[0].bounds.width, 20.0);
        assert!(matches!(
            resolved.items[0].kind,
            ResolvedKind::Rectangle { .. }
        ));
    }

    #[test]
    fn items_come_out_in_paint_order() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let first = doc.add_frame(layer, rect(0.0, 0.0, 1.0, 1.0));
        let second = doc.add_frame(layer, rect(0.0, 0.0, 2.0, 2.0));

        let resolved = resolve(&doc, &mut Shaper::new());

        assert_eq!(resolved.items[0].frame, first);
        assert_eq!(resolved.items[1].frame, second);
    }

    #[test]
    fn a_text_frame_resolves_with_text_shaped_to_the_frame_width() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(Story::new("Hello"));

        let mut frame = rect(0.0, 0.0, 500.0, 100.0);
        frame.kind = FrameKind::Text { story };
        doc.add_frame(layer, frame);

        let resolved = resolve(&doc, &mut Shaper::new());

        let ResolvedKind::Text { shaped, .. } = &resolved.items[0].kind else {
            panic!("expected text");
        };
        assert_eq!(shaped.glyph_count(), 5);
    }

    #[test]
    fn a_narrow_text_frame_wraps_because_the_frame_width_is_the_measure() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(Story::new("the quick brown fox jumps over"));

        let mut frame = rect(0.0, 0.0, 60.0, 100.0);
        frame.kind = FrameKind::Text { story };
        doc.add_frame(layer, frame);

        let resolved = resolve(&doc, &mut Shaper::new());

        let ResolvedKind::Text { shaped, .. } = &resolved.items[0].kind else {
            panic!("expected text");
        };
        assert!(
            shaped.lines.len() > 1,
            "the frame width must bound the text"
        );
    }

    #[test]
    fn a_hidden_layer_contributes_nothing() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(layer, rect(0.0, 0.0, 10.0, 10.0));
        doc.layers.get_mut(layer).expect("layer").visible = false;

        assert_eq!(resolve(&doc, &mut Shaper::new()).items.len(), 0);
    }

    #[test]
    fn a_text_frame_with_a_missing_story_does_not_paint() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(Story::new("gone"));
        doc.stories.remove(story);

        let mut frame = rect(0.0, 0.0, 100.0, 20.0);
        frame.kind = FrameKind::Text { story };
        doc.add_frame(layer, frame);

        assert!(resolve(&doc, &mut Shaper::new()).items.is_empty());
    }
}
