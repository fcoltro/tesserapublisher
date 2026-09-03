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

/// The narrowest a stroke may be drawn, in **document** units at `view`.
///
/// A one-point rule at 25% zoom is a quarter of a device pixel wide. Vello
/// renders that correctly — a quarter of the coverage — and the result is a
/// line that fades, breaks up along its length, and flickers as the view
/// moves, worst of all when it runs nearly straight across a row or column of
/// pixels and every pixel in the run makes the same wrong decision.
///
/// So a stroke is never asked for less than one device pixel of width. This is
/// what every drawing tool means by a hairline, and it applies to the screen
/// only: the PDF writer does not go through here, so an export keeps the width
/// the document actually specifies.
fn hairline(view: ViewTransform) -> f64 {
    const DEVICE_PIXELS: f64 = 1.0;
    if view.zoom.abs() < f64::EPSILON {
        return 0.0;
    }
    DEVICE_PIXELS / view.zoom.abs()
}

/// Build the scene for one page of a resolved document.
pub fn build_scene(resolved: &ResolvedDocument, view: ViewTransform, page: DocRect) -> Scene {
    let mut scene = Scene::new();
    let transform = view.to_affine();
    let hairline = hairline(view);
    let stroke_of = |width: f64| KurboStroke::new(width.max(hairline));

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
                        &stroke_of(s.width),
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
                        &stroke_of(s.width),
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
                    scene.stroke(&stroke_of(s.width), placed, to_peniko(&s.color), None, path);
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

    // --- hairlines ------------------------------------------------------

    fn view_at(zoom: f64) -> ViewTransform {
        ViewTransform {
            zoom,
            ..Default::default()
        }
    }

    #[test]
    fn a_stroke_is_never_asked_for_less_than_a_device_pixel() {
        // A one-point rule zoomed out to 25% is a quarter of a pixel wide.
        // Drawn honestly it fades and breaks up along its length, and a line
        // running nearly straight down a column of pixels breaks up the most,
        // because every pixel in the run makes the same wrong decision.
        let floor = hairline(view_at(0.25));
        assert!(
            (floor - 4.0).abs() < 1e-9,
            "a quarter-scale view needs 4 document units to make a pixel, got {floor}"
        );
        assert!(1.0_f64.max(floor) > 1.0, "a 1pt rule is widened at 25%");
    }

    #[test]
    fn zooming_in_never_widens_a_stroke() {
        // The floor is a floor. Past 1:1 it must do nothing at all, or every
        // hairline would fatten as you zoomed in.
        let width = 1.0_f64;
        for zoom in [1.0, 2.0, 8.0] {
            let drawn = width.max(hairline(view_at(zoom)));
            assert!(
                (drawn - width).abs() < 1e-9,
                "a 1pt stroke became {drawn} at {zoom}x"
            );
        }
    }

    #[test]
    fn a_thick_stroke_is_left_alone_however_far_out_the_view_is() {
        let drawn = 40.0_f64.max(hairline(view_at(0.1)));
        assert!((drawn - 40.0).abs() < 1e-9, "got {drawn}");
    }

    #[test]
    fn a_zero_zoom_does_not_produce_an_infinite_stroke() {
        // Nothing is visible at zero zoom, but a division by it would poison
        // the scene with a non-finite width rather than draw nothing.
        assert_eq!(hairline(view_at(0.0)), 0.0);
        assert!(hairline(view_at(0.0)).is_finite());
    }
}
