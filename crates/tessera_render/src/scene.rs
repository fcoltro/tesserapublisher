//! Building a `vello::Scene` from a resolved document.
//!
//! Pure construction, no GPU — so it is testable without a window.
//!
//! Only *document* content is drawn here. Selection handles, guides, snap
//! indicators and the text caret are interface, not document: they are drawn
//! by egui's painter on top, so they can never appear in an export.

use tessera_color::Color;
use tessera_geometry::{DocRect, ViewTransform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedKind};
use vello::kurbo::{Affine, Ellipse, Rect, Stroke as KurboStroke};
use vello::peniko::Fill;
use vello::peniko::color::{AlphaColor, Srgb};
use vello::{Glyph, Scene};

fn to_peniko(color: &Color) -> AlphaColor<Srgb> {
    let [r, g, b, a] = color.to_rgb_f32();
    AlphaColor::new([r, g, b, a])
}

/// Build the scene for one page of a resolved document.
pub fn build_scene(resolved: &ResolvedDocument, view: ViewTransform, page: DocRect) -> Scene {
    let mut scene = Scene::new();
    let transform = view.to_affine();

    // The page itself, so the document reads as paper rather than as objects
    // floating on the pasteboard.
    scene.fill(
        Fill::NonZero,
        transform,
        to_peniko(&Color::WHITE),
        None,
        &page.to_kurbo(),
    );

    for item in &resolved.items {
        let rect: Rect = item.bounds.to_kurbo();
        // Rotation is about the frame's own centre, so it composes as
        // translate-out, rotate, translate-back — applied before the camera.
        let transform = if item.rotation == 0.0 {
            transform
        } else {
            let c = item.bounds.center();
            transform
                * Affine::translate((c.x, c.y))
                * Affine::rotate(item.rotation.to_radians())
                * Affine::translate((-c.x, -c.y))
        };

        match &item.kind {
            ResolvedKind::Rectangle { fill, stroke } => {
                scene.fill(Fill::NonZero, transform, to_peniko(fill), None, &rect);
                if let Some(s) = stroke {
                    scene.stroke(
                        &KurboStroke::new(s.width),
                        transform,
                        to_peniko(&s.color),
                        None,
                        &rect,
                    );
                }
            }
            ResolvedKind::Ellipse { fill, stroke } => {
                let ellipse = Ellipse::from_rect(rect);
                scene.fill(Fill::NonZero, transform, to_peniko(fill), None, &ellipse);
                if let Some(s) = stroke {
                    scene.stroke(
                        &KurboStroke::new(s.width),
                        transform,
                        to_peniko(&s.color),
                        None,
                        &ellipse,
                    );
                }
            }
            ResolvedKind::Path { path, fill, stroke } => {
                // The path is frame-local, so it is placed by translating to
                // the frame's origin before the camera transform applies.
                let placed = transform * Affine::translate((item.bounds.x, item.bounds.y));
                if let Some(f) = fill {
                    scene.fill(Fill::NonZero, placed, to_peniko(f), None, path);
                }
                if let Some(s) = stroke {
                    scene.stroke(
                        &KurboStroke::new(s.width),
                        placed,
                        to_peniko(&s.color),
                        None,
                        path,
                    );
                }
            }

            ResolvedKind::Text { shaped, color } => {
                draw_text(&mut scene, transform, item.bounds, shaped, color);
            }
        }
    }

    scene
}

fn draw_text(
    scene: &mut Scene,
    transform: Affine,
    bounds: DocRect,
    shaped: &tessera_text::shape::ShapedText,
    color: &Color,
) {
    // One draw call per font. `FontData` is the very handle the shaper used —
    // the same `linebender_resource_handle` type peniko re-exports — so no
    // conversion happens and the renderer cannot pick different bytes than
    // the PDF writer will.
    for (index, font) in shaped.fonts.iter().enumerate() {
        let glyphs: Vec<Glyph> = shaped
            .lines
            .iter()
            .flat_map(|line| line.glyphs.iter())
            .filter(|g| g.font_index == index)
            .map(|g| Glyph {
                id: g.glyph_id,
                x: (bounds.x + g.x) as f32,
                y: (bounds.y + g.y) as f32,
            })
            .collect();

        if glyphs.is_empty() {
            continue;
        }

        scene
            .draw_glyphs(font)
            .font_size(shaped.font_size)
            .transform(transform)
            .brush(to_peniko(color))
            .draw(Fill::NonZero, glyphs.into_iter());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::ids::FrameId;
    use tessera_layout::resolve::ResolvedItem;
    use tessera_text::shape::Shaper;
    use tessera_text::story::Story;

    fn page() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 612.0,
            height: 792.0,
        }
    }

    fn empty_scene() -> Scene {
        build_scene(
            &ResolvedDocument::default(),
            ViewTransform::default(),
            page(),
        )
    }

    fn one_item(kind: ResolvedKind, bounds: DocRect) -> ResolvedDocument {
        ResolvedDocument {
            items: vec![ResolvedItem {
                frame: FrameId::default(),
                rotation: 0.0,
                bounds,
                kind,
            }],
        }
    }

    #[test]
    fn an_empty_document_still_paints_the_page() {
        assert!(
            !empty_scene().encoding().is_empty(),
            "the white page itself must be drawn"
        );
    }

    #[test]
    fn a_rectangle_adds_geometry_to_the_encoding() {
        let with_rect = build_scene(
            &one_item(
                ResolvedKind::Rectangle {
                    fill: Color::BLACK,
                    stroke: None,
                },
                DocRect {
                    x: 10.0,
                    y: 10.0,
                    width: 50.0,
                    height: 50.0,
                },
            ),
            ViewTransform::default(),
            page(),
        );

        assert!(
            with_rect.encoding().stream_offsets().path_data
                > empty_scene().encoding().stream_offsets().path_data
        );
    }

    #[test]
    fn a_stroke_encodes_more_than_a_fill_alone() {
        let bounds = DocRect {
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 50.0,
        };
        let filled = build_scene(
            &one_item(
                ResolvedKind::Rectangle {
                    fill: Color::BLACK,
                    stroke: None,
                },
                bounds,
            ),
            ViewTransform::default(),
            page(),
        );
        let stroked = build_scene(
            &one_item(
                ResolvedKind::Rectangle {
                    fill: Color::BLACK,
                    stroke: Some(tessera_document::nodes::Stroke {
                        color: Color::BLACK,
                        width: 2.0,
                    }),
                },
                bounds,
            ),
            ViewTransform::default(),
            page(),
        );

        assert!(
            stroked.encoding().stream_offsets().path_data
                > filled.encoding().stream_offsets().path_data
        );
    }

    #[test]
    fn an_ellipse_encodes_curves_rather_than_the_rectangle_it_fits() {
        let bounds = DocRect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 40.0,
        };
        let rect = build_scene(
            &one_item(
                ResolvedKind::Rectangle {
                    fill: Color::BLACK,
                    stroke: None,
                },
                bounds,
            ),
            ViewTransform::default(),
            page(),
        );
        let ellipse = build_scene(
            &one_item(
                ResolvedKind::Ellipse {
                    fill: Color::BLACK,
                    stroke: None,
                },
                bounds,
            ),
            ViewTransform::default(),
            page(),
        );

        assert_ne!(
            ellipse.encoding().stream_offsets().path_data,
            rect.encoding().stream_offsets().path_data,
            "an ellipse must not encode as its bounding rectangle"
        );
    }

    #[test]
    fn text_puts_exactly_its_glyphs_into_the_encoding() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("Hi"), 200.0);
        let expected = shaped.glyph_count();
        assert!(expected > 0, "the fixture must actually shape");

        let scene = build_scene(
            &one_item(
                ResolvedKind::Text {
                    shaped,
                    color: Color::BLACK,
                },
                DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 50.0,
                },
            ),
            ViewTransform::default(),
            page(),
        );

        // Glyphs are encoded as runs, not as path segments: Vello resolves
        // outlines later, so path_data does not move. Asserting on the glyph
        // stream directly is both correct and a stronger claim.
        let resources = &scene.encoding().resources;
        assert_eq!(resources.glyphs.len(), expected);
        assert_eq!(
            resources.glyph_runs.len(),
            1,
            "one run, since the fixture uses one font"
        );
    }

    #[test]
    fn text_with_no_glyphs_encodes_no_run_at_all() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new(""), 200.0);

        let scene = build_scene(
            &one_item(
                ResolvedKind::Text {
                    shaped,
                    color: Color::BLACK,
                },
                DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 50.0,
                },
            ),
            ViewTransform::default(),
            page(),
        );

        assert!(scene.encoding().resources.glyph_runs.is_empty());
    }
}
