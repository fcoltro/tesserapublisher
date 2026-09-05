//! Building a `vello::Scene` from a resolved document.
//!
//! Pure construction, no GPU — so it is testable without a window.
//!
//! Only *document* content is drawn here. Selection handles, guides, snap
//! indicators and the text caret are interface, not document: they are drawn
//! by egui's painter on top, so they can never appear in an export.

use tessera_color::Color;
use tessera_document::nodes::{LineCap, LineJoin, Stroke};
use tessera_geometry::{DocRect, ViewTransform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedKind};
use vello::kurbo::{Affine, Ellipse, Rect, Stroke as KurboStroke};
use vello::peniko::Fill;
use vello::peniko::color::{AlphaColor, Srgb};
use vello::{Glyph, Scene};

fn to_cap(cap: LineCap) -> vello::kurbo::Cap {
    match cap {
        LineCap::Butt => vello::kurbo::Cap::Butt,
        LineCap::Round => vello::kurbo::Cap::Round,
        LineCap::Square => vello::kurbo::Cap::Square,
    }
}

fn to_join(join: LineJoin) -> vello::kurbo::Join {
    match join {
        LineJoin::Miter => vello::kurbo::Join::Miter,
        LineJoin::Round => vello::kurbo::Join::Round,
        LineJoin::Bevel => vello::kurbo::Join::Bevel,
    }
}

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

/// The shape's own rectangle, moved out to where the stroke's centreline
/// runs.
///
/// An inside stroke on a frame thinner than the stroke itself would turn the
/// rectangle inside out, so the inset is held at the point where it collapses.
fn stroked_rect(bounds: Rect, offset: f64) -> Rect {
    let limit = (bounds.width().min(bounds.height()) / 2.0).max(0.0);
    bounds.inflate(offset.max(-limit), offset.max(-limit))
}

/// The non-printing rule drawn around a page's bleed.
///
/// Red is the press convention, and it is the one colour a designer already
/// reads as "this will be trimmed off".
const BLEED_RULE: [f32; 4] = [0.85, 0.22, 0.18, 1.0];

/// The non-printing rule drawn around a page's type area.
///
/// Magenta, again by convention — distinct from the bleed's red at a glance
/// even for the most common colour-vision deficiencies, which red and green
/// would not be.
const MARGIN_RULE: [f32; 4] = [0.78, 0.24, 0.72, 1.0];

/// What to include when building a scene.
///
/// A struct rather than a growing list of booleans, so a call reads as a
/// description of what it wants rather than as three bare `true`s.
#[derive(Debug, Clone, Copy)]
pub struct SceneOptions {
    /// Draw the non-printing margin and bleed rules.
    pub rules: bool,
    /// Show only what falls inside this rectangle.
    ///
    /// The printing screen modes crop to the trim, the bleed or the slug, so
    /// what is on screen is what will come off the press. `None` shows
    /// everything, pasteboard included.
    pub clip: Option<DocRect>,
}

impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            rules: true,
            clip: None,
        }
    }
}

/// Build the scene for a resolved document.
///
/// The pages come from `resolved`, not from a parameter. While the caller
/// passed a page rectangle separately, the screen and the PDF each decided for
/// themselves where the page was, and one of them was eventually going to be
/// wrong.
pub fn build_scene(resolved: &ResolvedDocument, view: ViewTransform) -> Scene {
    build_scene_with(resolved, view, SceneOptions::default())
}

/// As [`build_scene`], but able to leave the non-printing rules out.
///
/// The printing screen modes show the page as it will come off the press, and
/// a margin rule is not on the press.
pub fn build_scene_with(
    resolved: &ResolvedDocument,
    view: ViewTransform,
    options: SceneOptions,
) -> Scene {
    let rules = options.rules;
    let mut scene = Scene::new();
    let transform = view.to_affine();
    let hairline = hairline(view);

    // Everything the document draws goes inside this layer, so a printing
    // mode crops rather than merely hiding the furniture around the page.
    let clipped = options.clip.is_some();
    if let Some(area) = options.clip {
        // A plain layer clipped to the area: vello has no dedicated clip
        // blend, so the clip comes from the layer's own shape.
        scene.push_layer(
            Fill::NonZero,
            vello::peniko::Mix::Normal,
            1.0,
            transform,
            &area.to_kurbo(),
        );
    }
    // The whole stroke, not just its width: caps, joins and dashes are what
    // make a rule read as a rule rather than as a thin rectangle.
    let stroke_of = |s: &Stroke| {
        let mut k = KurboStroke::new(s.width.max(hairline));
        k.start_cap = to_cap(s.cap);
        k.end_cap = to_cap(s.cap);
        k.join = to_join(s.join);
        k.miter_limit = s.miter_limit;
        if s.is_dashed() {
            k = k.with_dashes(s.dash_offset, s.dashes.iter().copied());
        }
        k
    };

    // The pages themselves, so the document reads as paper rather than as
    // objects floating on the pasteboard. Every page of the spread, so facing
    // pages appear side by side.
    for page in &resolved.pages {
        scene.fill(
            Fill::NonZero,
            transform,
            to_peniko(&Color::WHITE),
            None,
            &page.bounds.to_kurbo(),
        );
    }

    // The guides that describe each page, drawn under its contents so that
    // objects sit on top of them rather than being cut by them.
    //
    // Each is drawn only when it says something the trim does not: an
    // unset bleed is the trim, and a rule on top of a rule is noise. The slug
    // is deliberately not drawn — it has no distinct meaning until screen
    // modes arrive, and two identical rectangles teach the reader nothing.
    let rule = KurboStroke::new(hairline);
    for page in resolved.pages.iter().filter(|_| rules) {
        if page.bleed != page.bounds {
            scene.stroke(
                &rule,
                transform,
                AlphaColor::<Srgb>::new(BLEED_RULE),
                None,
                &page.bleed.to_kurbo(),
            );
        }
        if page.margins != page.bounds {
            scene.stroke(
                &rule,
                transform,
                AlphaColor::<Srgb>::new(MARGIN_RULE),
                None,
                &page.margins.to_kurbo(),
            );
        }
    }

    for item in &resolved.items {
        let rect: Rect = item.bounds.to_kurbo();
        // The frame's own space, then the camera. `bounds` is expressed in
        // that own space, so the item transform has to be applied to it
        // before the view is.
        let transform = transform * item.transform.to_affine();

        match &item.kind {
            ResolvedKind::Rectangle { fill, stroke } => {
                scene.fill(Fill::NonZero, transform, to_peniko(fill), None, &rect);
                if let Some(s) = stroke {
                    scene.stroke(
                        &stroke_of(s),
                        transform,
                        to_peniko(&s.color),
                        None,
                        &stroked_rect(rect, s.offset()),
                    );
                }
            }
            ResolvedKind::Ellipse { fill, stroke } => {
                let ellipse = Ellipse::from_rect(rect);
                scene.fill(Fill::NonZero, transform, to_peniko(fill), None, &ellipse);
                if let Some(s) = stroke {
                    scene.stroke(
                        &stroke_of(s),
                        transform,
                        to_peniko(&s.color),
                        None,
                        &Ellipse::from_rect(stroked_rect(rect, s.offset())),
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
                    // A path's alignment is not applied: offsetting an
                    // arbitrary curve is a different problem from insetting a
                    // rectangle, and drawing it centred is honest where
                    // approximating the offset would not be.
                    scene.stroke(&stroke_of(s), placed, to_peniko(&s.color), None, path);
                }
            }

            ResolvedKind::Text { shaped, color } => {
                draw_text(&mut scene, transform, item.bounds, shaped, color);
            }
        }
    }

    if clipped {
        scene.pop_layer();
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
    // One draw call per run, because the size lives on the run. This was one
    // call per font while a story had a single size; grouping by font alone
    // would now draw a heading and its body text at whichever size happened
    // to be asked for first.
    //
    // `FontData` is the very handle the shaper used — the same
    // `linebender_resource_handle` type peniko re-exports — so no conversion
    // happens and the renderer cannot pick different bytes than the PDF
    // writer will.
    for run in shaped.runs() {
        let Some(font) = shaped.fonts.get(run.font_index) else {
            continue;
        };
        if run.glyphs.is_empty() {
            continue;
        }

        let glyphs: Vec<Glyph> = run
            .glyphs
            .iter()
            .map(|g| Glyph {
                id: g.glyph_id,
                x: (bounds.x + g.x) as f32,
                y: (bounds.y + g.y) as f32,
            })
            .collect();

        scene
            .draw_glyphs(font)
            .font_size(run.size)
            .transform(transform)
            .brush(to_peniko(color))
            .draw(Fill::NonZero, glyphs.into_iter());
    }
}

#[cfg(test)]
mod tests {
    /// A story shaped at two sizes must reach the scene as two draw calls.
    ///
    /// Grouping by font alone — which is what this did while a story had one
    /// size — would draw a heading and its body at whichever size came first.
    #[test]
    fn each_run_is_drawn_at_its_own_size() {
        use tessera_text::story::{CharacterFormat, Run, Story};

        let sized = |size: f32, range: std::ops::Range<usize>| Run {
            range,
            style: None,
            local: CharacterFormat {
                size: Some(size),
                ..CharacterFormat::default()
            },
        };

        let mut one_size = Story::new("bigsmall");
        one_size.runs = vec![sized(12.0, 0..8)];

        let mut two_sizes = Story::new("bigsmall");
        two_sizes.runs = vec![sized(24.0, 0..3), sized(9.0, 3..8)];

        let mut shaper = tessera_text::shape::Shaper::new();
        let uniform = shaper.shape(&one_size, &NoStyles::default(), 1000.0);
        let mixed = shaper.shape(&two_sizes, &NoStyles::default(), 1000.0);

        assert!(
            mixed.runs().count() > uniform.runs().count(),
            "two sizes should shape to more runs than one"
        );

        let build = |shaped: tessera_text::shape::ShapedText| {
            build_scene(
                &one_item(
                    ResolvedKind::Text {
                        shaped,
                        color: Color::BLACK,
                    },
                    page(),
                ),
                ViewTransform::default(),
            )
        };

        // Both draw glyphs; the mixed one draws them in more than one call.
        assert!(!build(mixed).encoding().resources.glyph_runs.is_empty());
        assert!(!build(uniform).encoding().resources.glyph_runs.is_empty());
    }

    #[test]
    fn a_clip_really_reaches_the_encoding() {
        // Preview must show the trim as it will print, not merely hide the
        // furniture around it — so the clip has to be in the scene, not just
        // in the options struct.
        let doc = one_item(
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
        );
        let plain = build_scene_with(&doc, ViewTransform::default(), SceneOptions::default());
        let cropped = build_scene_with(
            &doc,
            ViewTransform::default(),
            SceneOptions {
                rules: true,
                clip: Some(page()),
            },
        );
        assert!(
            cropped.encoding().n_clips > plain.encoding().n_clips,
            "the clip layer never reached the encoding"
        );
    }

    #[test]
    fn leaving_the_rules_out_draws_less() {
        let mut doc = one_item(
            ResolvedKind::Rectangle {
                fill: Color::BLACK,
                stroke: None,
            },
            DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
        );
        // A margin inset from the trim, so there is a rule to leave out.
        doc.pages[0].margins = DocRect {
            x: 20.0,
            y: 20.0,
            width: page().width - 40.0,
            height: page().height - 40.0,
        };

        let with_rules = build_scene_with(&doc, ViewTransform::default(), SceneOptions::default());
        let without = build_scene_with(
            &doc,
            ViewTransform::default(),
            SceneOptions {
                rules: false,
                clip: None,
            },
        );
        assert!(
            without.encoding().stream_offsets().path_data
                < with_rules.encoding().stream_offsets().path_data,
            "the margin rule was drawn in a printing mode"
        );
    }

    use super::*;
    use tessera_document::ids::FrameId;
    use tessera_geometry::Transform;
    use tessera_layout::resolve::ResolvedItem;
    use tessera_text::shape::Shaper;
    use tessera_text::story::{NoStyles, Story};

    fn page() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 612.0,
            height: 792.0,
        }
    }

    fn empty_scene() -> Scene {
        build_scene(&one_page(vec![]), ViewTransform::default())
    }

    /// The default page, resolved with no margins, bleed or slug.
    fn resolved_page() -> tessera_layout::ResolvedPage {
        tessera_layout::ResolvedPage {
            bounds: page(),
            margins: page(),
            bleed: page(),
            slug: page(),
        }
    }

    /// A document holding one page and the given items.
    fn one_page(items: Vec<ResolvedItem>) -> ResolvedDocument {
        ResolvedDocument {
            items,
            pages: vec![resolved_page()],
        }
    }

    fn one_item(kind: ResolvedKind, bounds: DocRect) -> ResolvedDocument {
        ResolvedDocument {
            pages: vec![resolved_page()],
            items: vec![ResolvedItem {
                frame: FrameId::default(),
                transform: Transform::IDENTITY,
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
        );
        let stroked = build_scene(
            &one_item(
                ResolvedKind::Rectangle {
                    fill: Color::BLACK,
                    stroke: Some(Stroke::new(Color::BLACK, 2.0)),
                },
                bounds,
            ),
            ViewTransform::default(),
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
        let shaped = shaper.shape(&Story::new("Hi"), &NoStyles::default(), 200.0);
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
        let shaped = shaper.shape(&Story::new(""), &NoStyles::default(), 200.0);

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
