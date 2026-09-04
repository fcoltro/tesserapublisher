//! Resolves a document into drawable items.
//!
//! Both the screen renderer and the PDF writer consume the output of this
//! module, so neither re-derives geometry nor re-shapes text. That shared
//! source is what keeps an export from drifting away from the screen.

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::ids::FrameId;
use tessera_document::nodes::{FrameKind, Stroke};
use tessera_document::path::fit_to_bounds;
use tessera_geometry::{DocRect, Transform};
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
    /// The frame's own space, mapped onto the document. Both the renderer
    /// and the PDF writer apply this the same way, from this one value.
    pub transform: Transform,
    pub kind: ResolvedKind,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedDocument {
    /// Back to front. The last item paints on top.
    pub items: Vec<ResolvedItem>,
    /// Every page, with the rectangles that describe it.
    ///
    /// Computed once here so that the screen and the PDF cannot disagree
    /// about where the trim is. While each computed its own, one of them was
    /// eventually going to be wrong.
    pub pages: Vec<ResolvedPage>,
}

/// One page, with the rectangles that describe it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPage {
    /// The trim: the paper itself.
    pub bounds: DocRect,
    /// The type area, inset by the margins.
    pub margins: DocRect,
    /// The trim plus its bleed.
    pub bleed: DocRect,
    /// The trim plus its slug.
    pub slug: DocRect,
}

/// Resolve every visible frame, in paint order.
pub fn resolve(doc: &Document, shaper: &mut Shaper) -> ResolvedDocument {
    let pages = doc
        .page_ids()
        .filter_map(|id| {
            Some(ResolvedPage {
                bounds: doc.pages.get(id)?.bounds,
                margins: doc.margin_rect(id)?,
                bleed: doc.bleed_rect(id)?,
                slug: doc.slug_rect(id)?,
            })
        })
        .collect();

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
                path: fit_to_bounds(path, frame.bounds),
                // An open path with no explicit stroke would be invisible, so
                // a path frame's fill is treated as its stroke colour when it
                // has no stroke of its own.
                fill: None,
                stroke: Some(
                    frame
                        .stroke
                        .clone()
                        .unwrap_or_else(|| Stroke::new(frame.fill.clone(), 1.0)),
                ),
            },

            // A group draws nothing of its own, and paint_order already
            // expanded it into its children, so it never reaches here.
            FrameKind::Group(_) => continue,

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
            transform: frame.transform,
            kind,
        });
    }

    ResolvedDocument { items, pages }
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
            transform: Transform::IDENTITY,
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

    fn path_frame(bounds: DocRect, path: kurbo::BezPath) -> Frame {
        Frame {
            bounds,
            kind: FrameKind::Path(path),
            transform: Transform::IDENTITY,
            fill: Color::BLACK,
            stroke: None,
        }
    }

    /// A diagonal line filling a 10x10 box.
    fn diagonal() -> kurbo::BezPath {
        let mut p = kurbo::BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 10.0));
        p
    }

    #[test]
    fn a_path_is_scaled_to_fill_its_frame() {
        use kurbo::Shape as _;
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        // Same path, but a frame twice as wide and three times as tall.
        doc.add_frame(
            layer,
            path_frame(
                DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 30.0,
                },
                diagonal(),
            ),
        );

        let resolved = resolve(&doc, &mut Shaper::new());
        let ResolvedKind::Path { path, .. } = &resolved.items[0].kind else {
            panic!("expected a path");
        };
        let b = path.bounding_box();

        assert!((b.width() - 20.0).abs() < 1e-9, "width was {}", b.width());
        assert!(
            (b.height() - 30.0).abs() < 1e-9,
            "height was {}",
            b.height()
        );
    }

    #[test]
    fn a_path_that_already_fits_is_left_alone() {
        use kurbo::Shape as _;
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(
            layer,
            path_frame(
                DocRect {
                    x: 5.0,
                    y: 5.0,
                    width: 10.0,
                    height: 10.0,
                },
                diagonal(),
            ),
        );

        let resolved = resolve(&doc, &mut Shaper::new());
        let ResolvedKind::Path { path, .. } = &resolved.items[0].kind else {
            panic!("expected a path");
        };

        assert_eq!(path.bounding_box(), diagonal().bounding_box());
    }

    #[test]
    fn a_horizontal_line_scales_across_but_is_not_flattened_further() {
        use kurbo::Shape as _;
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut flat = kurbo::BezPath::new();
        flat.move_to((0.0, 0.0));
        flat.line_to((10.0, 0.0));

        doc.add_frame(
            layer,
            path_frame(
                DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 50.0,
                    height: 0.0,
                },
                flat,
            ),
        );

        let resolved = resolve(&doc, &mut Shaper::new());
        let ResolvedKind::Path { path, .. } = &resolved.items[0].kind else {
            panic!("expected a path");
        };
        let b = path.bounding_box();

        assert!((b.width() - 50.0).abs() < 1e-9, "width was {}", b.width());
        assert!(
            b.height().abs() < 1e-9,
            "an axis with no extent must not blow up"
        );
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;
    use tessera_document::nodes::{Insets, Margins};

    #[test]
    fn resolving_carries_every_pages_rectangles() {
        let mut doc = Document::new();
        doc.setup.margins = Margins::uniform(36.0);
        doc.setup.bleed = Insets::uniform(9.0);

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        assert_eq!(resolved.pages.len(), doc.page_ids().count());
        let page = resolved.pages[0];
        assert_eq!(page.margins.width, page.bounds.width - 72.0);
        assert_eq!(page.bleed.width, page.bounds.width + 18.0);
        assert_eq!(page.slug, page.bounds, "no slug set means no slug drawn");
    }
}
