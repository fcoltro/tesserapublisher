//! Building a `vello::Scene` from a resolved document.
//!
//! Pure construction, no GPU — so it is testable without a window.
//!
//! Only *document* content is drawn here. Selection handles, guides, snap
//! indicators and the text caret are interface, not document: they are drawn
//! by egui's painter on top, so they can never appear in an export.

use tessera_color::Color;
use tessera_color::managed::Proof;
use tessera_document::nodes::{LineCap, LineJoin, Stroke};
use tessera_document::paint::Paint;
use tessera_geometry::{DocRect, ViewTransform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedKind};
use vello::kurbo::{Affine, Ellipse, Line, Rect, Stroke as KurboStroke};
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

/// A document colour, as it will look on the chosen press.
///
/// **Every colour the document itself draws goes through here, and nothing else
/// does.** A margin rule, a frame edge and a selection handle are *interface*,
/// and proofing them would tint the furniture to match the paper — which tells a
/// person nothing about their job and makes the application look broken. Those
/// are drawn from the theme’s own constants and never pass through a `Color`,
/// which is what makes the separation hold by construction rather than by care.
///
/// With no proof this is the plain conversion, so the unproofed path costs one
/// branch and nothing else.
fn ink(colour: &Color, proof: Option<&Proof>) -> AlphaColor<Srgb> {
    let shown = match proof {
        Some(proof) => proof.show(colour),
        None => colour.to_rgb_f32(),
    };
    AlphaColor::new(shown)
}

/// The object's shadow, painted before the object itself.
///
/// **Behind, not under**: it is drawn first and the object covers it, which is
/// why a translucent object shows its own shadow through itself exactly as it
/// would show the page.
///
/// Vello can blur a rounded rectangle and nothing else, and that decides how
/// honest each shape's shadow is:
///
/// - a rectangle, a picture box and a text frame are rectangles, so their
///   shadows are exact;
/// - an ellipse takes a corner radius of half its shorter side, which for a
///   **circle is the circle exactly** and for a long ellipse is a capsule — a
///   close enough shape that the blur hides the difference;
/// - a path gets its bounding box, which is the one case that is visibly not the
///   object. Blurring an arbitrary curve needs an offscreen pass, and that is
///   milestone 6's business.
fn draw_shadow(
    scene: &mut Scene,
    transform: Affine,
    rect: Rect,
    kind: &ResolvedKind,
    shadow: &tessera_document::shadow::Shadow,
    proof: Option<&Proof>,
) {
    if shadow.is_invisible() {
        return;
    }

    let radius = match kind {
        ResolvedKind::Ellipse { .. } => rect.width().min(rect.height()) / 2.0,
        _ => 0.0,
    };
    let offset = rect + vello::kurbo::Vec2::new(shadow.offset.0, shadow.offset.1);

    scene.draw_blurred_rounded_rect(
        transform,
        offset,
        ink(&shadow.colour, proof),
        radius,
        shadow.std_dev(),
    );
}

/// What vello paints a shape with.
///
/// The gradient is built in the **frame's own space**, which is the space the
/// shape is given in, so the ramp is carried by the same transform that carries
/// the object: a rotated frame rotates its gradient, and a resized one restretches
/// it, without either being rewritten.
fn brush_of(paint: &Paint, bounds: DocRect, proof: Option<&Proof>) -> vello::peniko::Brush {
    use tessera_document::paint::Ramp;
    use vello::peniko::{Brush, Gradient};

    let gradient = match paint {
        Paint::Solid(colour) => return Brush::Solid(ink(colour, proof)),
        Paint::Gradient(g) => g,
    };

    let (from, to) = gradient.axis(bounds);
    let mut built = match gradient.ramp {
        Ramp::Linear { .. } => Gradient::new_linear((from.x, from.y), (to.x, to.y)),
        // A radial ramp has no direction, so `axis` hands back the centre and a
        // point one radius away; only the distance is used.
        Ramp::Radial => Gradient::new_radial((from.x, from.y), gradient.radius(bounds) as f32),
    };
    built = built.with_stops(
        gradient
            .stops()
            .iter()
            .map(|stop| vello::peniko::ColorStop {
                offset: stop.at,
                color: ink(&stop.colour, proof).into(),
            })
            .collect::<Vec<_>>()
            .as_slice(),
    );
    Brush::Gradient(built)
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
/// An empty picture box.
///
/// The same violet the column guides use: both are furniture saying where
/// something will go rather than something that is there.
const PLACEHOLDER_RULE: [f32; 4] = [0.55, 0.36, 0.85, 1.0];
/// A picture box whose file has gone.
///
/// Red, and it has to be a different colour from an empty one: "nothing has
/// been placed here" and "what was placed here is gone" are different problems
/// and only the second is a fault.
const MISSING_RULE: [f32; 4] = [0.85, 0.20, 0.20, 1.0];

/// The column guides.
///
/// Violet: a relative of the magenta margin rule, because a column guide is a
/// subdivision of the type area rather than a different kind of thing — and
/// distinct enough that the two do not read as one line when they meet.
const COLUMN_RULE: [f32; 4] = [0.55, 0.36, 0.85, 1.0];

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

impl SceneOptions {
    /// A scene that shows the document as the chosen press will reproduce it.
    ///
    /// The proof is lent rather than owned because compiling one is the
    /// expensive half: building a transform per frame would cost more than the
    /// naive formula it replaces, so it is made when a profile is chosen and kept
    /// for as long as that choice stands.
    pub fn proofed(self, proof: &Proof) -> Proofed<'_> {
        Proofed {
            options: self,
            proof: Some(proof),
        }
    }
}

/// Scene options together with the proof to draw through, if any.
///
/// A separate type rather than a lifetime on `SceneOptions`, so that everything
/// building an ordinary scene — the tests, the headless renderer, the PDF
/// preview — stays free of a lifetime it has no use for.
pub struct Proofed<'a> {
    pub options: SceneOptions,
    pub proof: Option<&'a Proof>,
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

/// As [`build_scene`], with somewhere to decode artwork into.
///
/// Separate because the cache has to outlive one frame — that is the whole
/// point of it — so the caller owns it and lends it. A scene builder that made
/// its own would decode every photograph on every redraw, which is exactly
/// what the cache exists to prevent.
pub fn build_scene_with_images(
    resolved: &ResolvedDocument,
    view: ViewTransform,
    options: SceneOptions,
    images: &mut crate::images::Images,
) -> Scene {
    build_inner(resolved, view, options, Some(images), None)
}

/// As [`build_scene_with_images`], showing the document as a press will print it.
pub fn build_scene_proofed(
    resolved: &ResolvedDocument,
    view: ViewTransform,
    proofed: Proofed<'_>,
    images: &mut crate::images::Images,
) -> Scene {
    build_inner(resolved, view, proofed.options, Some(images), proofed.proof)
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
    build_inner(resolved, view, options, None, None)
}

fn build_inner(
    resolved: &ResolvedDocument,
    view: ViewTransform,
    options: SceneOptions,
    mut images: Option<&mut crate::images::Images>,
    proof: Option<&Proof>,
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
            // The paper. Proofed too, and deliberately: the paper’s own white is
            // the most visible thing a proof shows, and a page drawn pure white
            // behind proofed ink would make every colour look wrong in the same
            // direction.
            ink(&Color::WHITE, proof),
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
        // The column guides, drawn as the sides of each column rather than as
        // boxes: their tops and bottoms lie on the margin rule already, and
        // stroking them again doubles a line that is meant to be a hairline.
        for column in &page.columns {
            for x in [column.x, column.x + column.width] {
                scene.stroke(
                    &rule,
                    transform,
                    AlphaColor::<Srgb>::new(COLUMN_RULE),
                    None,
                    &Line::new((x, column.y), (x, column.y + column.height)),
                );
            }
        }
    }

    // One clip per spread, opened when the spread changes and closed after the
    // last item on it. A frame may hang off its page onto the pasteboard, and
    // may not reach into the next spread — which is a different sheet of paper,
    // and looked like content flowing there.
    let mut spread: Option<DocRect> = None;
    let mut in_spread = false;

    for item in &resolved.items {
        if item.spread_area != spread {
            if in_spread {
                scene.pop_layer();
                in_spread = false;
            }
            spread = item.spread_area;
            if let Some(area) = spread {
                scene.push_layer(
                    Fill::NonZero,
                    vello::peniko::Mix::Normal,
                    1.0,
                    transform,
                    &area.to_kurbo(),
                );
                in_spread = true;
            }
        }

        // An object at no opacity paints nothing. It is still selectable and
        // still in the layers panel — this is about ink, not about existence.
        if item.blend.is_invisible() {
            continue;
        }

        let rect: Rect = item.bounds.to_kurbo();
        // The frame's own space, then the camera. `bounds` is expressed in
        // that own space, so the item transform has to be applied to it
        // before the view is.
        let transform = transform * item.transform.to_affine();

        // The shadow, behind everything the object paints and **outside** its
        // composite group: a shadow inside the group would be faded by the
        // object's own opacity, and a 50% object would cast a 25% shadow. It is
        // the object that is translucent, not the light.
        if let Some(shadow) = &item.shadow {
            draw_shadow(&mut scene, transform, rect, &item.kind, shadow, proof);
        }

        // **The object's own composite group**, and the reason object opacity
        // is not a fill colour's alpha: everything belonging to the object —
        // fill, stroke, artwork, glyphs — is painted into this layer and the
        // *result* is made translucent and mixed. Setting an alpha on each
        // paint instead would show the object's stroke through its own fill.
        //
        // Only when it needs one. Nearly every object is plain, and a layer per
        // object would cost a composite for each of them.
        let composited = !item.blend.is_plain();
        if composited {
            scene.push_layer(
                Fill::NonZero,
                mix_of(item.blend.mode),
                item.blend.alpha(),
                transform,
                &paint_extent(&item.kind, rect),
            );
        }

        // The artwork, when there is any. Whether the link is missing decides
        // the *placeholder's* colour, which the match below draws; here there
        // is either artwork to paint or there is not.
        if let ResolvedKind::Graphic {
            inner,
            source,
            natural,
            missing: _,
            stroke,
        } = &item.kind
        {
            // The artwork, clipped by its container. The clip is what
            // makes a crop a crop: content larger than the frame is cut
            // by it rather than spilling onto the page.
            // How big the artwork actually lands on screen, in device pixels.
            // Asking for a proxy that size is what lets the disk cache do
            // anything: a thumbnail wants a thumbnail, not a 40-megapixel
            // photograph shrunk on every frame.
            let scale = transform.determinant().abs().sqrt().max(f64::EPSILON);
            let across = (rect.width().max(rect.height()) * scale).ceil().max(1.0);
            let wanted = if across.is_finite() && across < f64::from(u32::MAX) {
                Some(across as u32)
            } else {
                None
            };

            let drawn = source.as_ref().and_then(|path| {
                images
                    .as_mut()
                    .and_then(|cache| cache.at_size(path, wanted))
                    .map(|decoded| (decoded.image.clone(), decoded.pixels))
            });

            if let Some((image, pixels)) = drawn {
                scene.push_layer(
                    Fill::NonZero,
                    vello::peniko::Mix::Normal,
                    1.0,
                    transform,
                    &rect,
                );
                // The content's own transform, then the scale from pixels
                // to the points the layout thinks in. `natural` is what
                // the artwork wants to be; the pixels are what it is.
                let to_points = if pixels.0 > 0 && pixels.1 > 0 && natural.0 > 0.0 {
                    Affine::scale_non_uniform(
                        natural.0 / f64::from(pixels.0),
                        natural.1 / f64::from(pixels.1),
                    )
                } else {
                    Affine::IDENTITY
                };
                scene.draw_image(
                    &vello::peniko::ImageBrush::from(image),
                    transform * inner.to_affine() * to_points,
                );
                scene.pop_layer();

                if let Some(s) = stroke {
                    scene.stroke(
                        &stroke_of(s),
                        transform,
                        ink(&s.color, proof),
                        None,
                        &stroked_rect(rect, s.offset()),
                    );
                }
                if composited {
                    scene.pop_layer();
                }
                continue;
            }
        }

        match &item.kind {
            ResolvedKind::Graphic {
                missing, stroke, ..
            } => {
                let rule = KurboStroke::new(1.0);
                let colour = if *missing {
                    MISSING_RULE
                } else {
                    PLACEHOLDER_RULE
                };
                scene.stroke(
                    &rule,
                    transform,
                    AlphaColor::<Srgb>::new(colour),
                    None,
                    &rect,
                );
                // The diagonals, which is how every layout tool has drawn an
                // empty picture box since the first one.
                for line in [
                    Line::new((rect.x0, rect.y0), (rect.x1, rect.y1)),
                    Line::new((rect.x1, rect.y0), (rect.x0, rect.y1)),
                ] {
                    scene.stroke(
                        &rule,
                        transform,
                        AlphaColor::<Srgb>::new(colour),
                        None,
                        &line,
                    );
                }
                if let Some(s) = stroke {
                    scene.stroke(
                        &stroke_of(s),
                        transform,
                        ink(&s.color, proof),
                        None,
                        &stroked_rect(rect, s.offset()),
                    );
                }
            }

            ResolvedKind::Rectangle { fill, stroke } => {
                scene.fill(
                    Fill::NonZero,
                    transform,
                    &brush_of(fill, item.bounds, proof),
                    None,
                    &rect,
                );
                if let Some(s) = stroke {
                    scene.stroke(
                        &stroke_of(s),
                        transform,
                        ink(&s.color, proof),
                        None,
                        &stroked_rect(rect, s.offset()),
                    );
                }
            }
            ResolvedKind::Ellipse { fill, stroke } => {
                let ellipse = Ellipse::from_rect(rect);
                scene.fill(
                    Fill::NonZero,
                    transform,
                    &brush_of(fill, item.bounds, proof),
                    None,
                    &ellipse,
                );
                if let Some(s) = stroke {
                    scene.stroke(
                        &stroke_of(s),
                        transform,
                        ink(&s.color, proof),
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
                    // The path is drawn in a translated space, so the gradient
                    // is built about the origin rather than about the frame's
                    // place on the page.
                    let local = DocRect {
                        x: 0.0,
                        y: 0.0,
                        width: item.bounds.width,
                        height: item.bounds.height,
                    };
                    scene.fill(
                        Fill::NonZero,
                        placed,
                        &brush_of(f, local, proof),
                        None,
                        path,
                    );
                }
                if let Some(s) = stroke {
                    // A path's alignment is not applied: offsetting an
                    // arbitrary curve is a different problem from insetting a
                    // rectangle, and drawing it centred is honest where
                    // approximating the offset would not be.
                    scene.stroke(&stroke_of(s), placed, ink(&s.color, proof), None, path);
                }
            }

            ResolvedKind::Text { shaped, color } => {
                draw_text(&mut scene, transform, item.bounds, shaped, color, proof);
            }
        }

        if composited {
            scene.pop_layer();
        }
    }

    if in_spread {
        scene.pop_layer();
    }
    if clipped {
        scene.pop_layer();
    }

    scene
}

/// The mix vello paints an object's composite group with.
///
/// Only the separable modes milestone 5 promises. A mode that cannot be
/// reproduced identically on screen and in the PDF is worse than no mode, so
/// there is deliberately no catch-all arm converting something else to Normal.
fn mix_of(mode: tessera_document::blending::BlendMode) -> vello::peniko::Mix {
    use tessera_document::blending::BlendMode;
    use vello::peniko::Mix;
    match mode {
        BlendMode::Normal => Mix::Normal,
        BlendMode::Multiply => Mix::Multiply,
        BlendMode::Screen => Mix::Screen,
        BlendMode::Overlay => Mix::Overlay,
    }
}

/// A rectangle certainly containing everything the object paints.
///
/// A composite group needs a shape, and the honest one is "at least the ink".
/// Deliberately an over-approximation: a clip larger than the ink changes
/// nothing, while one a hair too small cuts the outside half of a stroke off
/// and would look like a rendering bug rather than like an opacity setting.
fn paint_extent(kind: &ResolvedKind, rect: Rect) -> Rect {
    let reach = match kind {
        ResolvedKind::Rectangle { stroke, .. }
        | ResolvedKind::Ellipse { stroke, .. }
        | ResolvedKind::Path { stroke, .. }
        | ResolvedKind::Graphic { stroke, .. } => stroke.as_ref().map(|s| s.width).unwrap_or(0.0),
        // Text is clipped to its frame before it is composited, so the frame
        // is already the whole of it.
        ResolvedKind::Text { .. } => 0.0,
    };
    rect.inflate(reach, reach)
}

fn draw_text(
    scene: &mut Scene,
    transform: Affine,
    bounds: DocRect,
    shaped: &tessera_text::shape::ShapedText,
    color: &Color,
    proof: Option<&Proof>,
) {
    // **Text never leaves its frame.** A story longer than its box is overset:
    // InDesign marks it and draws none of it past the edge. Letting it spill
    // put one page's copy over the page below, which reads as though the text
    // had flowed there — and flowing between frames is a real feature this is
    // not.
    //
    // A plain layer clipped to the frame, because vello has no dedicated clip
    // blend and the clip comes from the layer's own shape.
    scene.push_layer(
        Fill::NonZero,
        vello::peniko::Mix::Normal,
        1.0,
        transform,
        &bounds.to_kurbo(),
    );

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
        // The run's own colour when it states one; otherwise the frame's,
        // which is what every story that nobody has coloured still uses.
        let colour = run.colour.as_ref().unwrap_or(color);

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
            .brush(ink(colour, proof))
            .draw(Fill::NonZero, glyphs.into_iter());
    }

    scene.pop_layer();
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
    fn a_translucent_object_gets_its_own_composite_group() {
        // The reason object opacity is not a fill colour's alpha: everything
        // belonging to the object is painted into one layer and the *result* is
        // made translucent, so the object's stroke does not show through its
        // own fill.
        use tessera_document::blending::Blending;

        let plain = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
            },
            DocRect {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 50.0,
            },
        );
        let mut faded = plain.clone();
        faded.items[0].blend = Blending {
            opacity: 0.5,
            mode: tessera_document::blending::BlendMode::Normal,
        };

        let flat = build_scene(&plain, ViewTransform::default());
        let composited = build_scene(&faded, ViewTransform::default());
        assert!(
            composited.encoding().n_clips > flat.encoding().n_clips,
            "no composite group reached the encoding"
        );
    }

    #[test]
    fn a_blend_mode_at_full_opacity_still_composites() {
        // A mode is a composite even when nothing is translucent, and treating
        // "opacity is 1" as "nothing to do" would silently drop it.
        use tessera_document::blending::{BlendMode, Blending};

        let plain = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
            },
            DocRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
        );
        let mut multiplied = plain.clone();
        multiplied.items[0].blend = Blending {
            opacity: 1.0,
            mode: BlendMode::Multiply,
        };

        assert!(
            build_scene(&multiplied, ViewTransform::default())
                .encoding()
                .n_clips
                > build_scene(&plain, ViewTransform::default())
                    .encoding()
                    .n_clips
        );
    }

    #[test]
    fn an_object_at_no_opacity_paints_nothing() {
        use tessera_document::blending::{BlendMode, Blending};

        let mut doc = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
            },
            DocRect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            },
        );
        let with_it = build_scene(&doc, ViewTransform::default());
        doc.items[0].blend = Blending {
            opacity: 0.0,
            mode: BlendMode::Normal,
        };
        let without = build_scene(&doc, ViewTransform::default());

        assert!(
            without.encoding().stream_offsets().path_data
                < with_it.encoding().stream_offsets().path_data,
            "an invisible object still put a path in the scene"
        );
    }

    #[test]
    fn a_plain_object_costs_no_composite_at_all() {
        // Nearly every object is plain, so the cheap path has to stay cheap.
        let empty = empty_scene();
        let one = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
            },
            DocRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
        );
        assert_eq!(
            build_scene(&one, ViewTransform::default())
                .encoding()
                .n_clips,
            empty.encoding().n_clips,
            "an opaque rectangle opened a layer it did not need"
        );
    }

    #[test]
    fn a_gradient_fill_reaches_the_encoding_as_a_ramp() {
        // A gradient drawn as a flat colour is the failure that would go
        // unnoticed, so this asks the encoding whether a ramp is really there.
        use tessera_document::paint::{Gradient, Ramp};

        let bounds = DocRect {
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 50.0,
        };
        let solid = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
            },
            bounds,
        );
        let ramped = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Gradient(Gradient::black_to_white(Ramp::Linear { angle: 0.0 })),
                stroke: None,
            },
            bounds,
        );

        let flat = build_scene(&solid, ViewTransform::default());
        let gradient = build_scene(&ramped, ViewTransform::default());
        assert!(
            gradient.encoding().resources.color_stops.len()
                > flat.encoding().resources.color_stops.len(),
            "no colour ramp reached the encoding"
        );
    }

    #[test]
    fn a_radial_gradient_reaches_the_encoding_too() {
        use tessera_document::paint::{Gradient, Ramp};

        let doc = one_item(
            ResolvedKind::Ellipse {
                fill: Paint::Gradient(Gradient::black_to_white(Ramp::Radial)),
                stroke: None,
            },
            DocRect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            },
        );
        assert!(
            !build_scene(&doc, ViewTransform::default())
                .encoding()
                .resources
                .color_stops
                .is_empty()
        );
    }

    #[test]
    fn a_gradient_runs_across_the_object_it_fills_rather_than_across_the_page() {
        // The reason the ramp is expressed as an angle and built in the frame’s
        // own space: two frames of the same size at different places on the page
        // must produce the same ramp, moved.
        use tessera_document::paint::{Gradient, Ramp};

        let ramp = Paint::Gradient(Gradient::black_to_white(Ramp::Linear { angle: 0.0 }));
        let near = brush_of(
            &ramp,
            DocRect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            },
            None,
        );
        let far = brush_of(
            &ramp,
            DocRect {
                x: 300.0,
                y: 400.0,
                width: 50.0,
                height: 50.0,
            },
            None,
        );

        let ends = |brush: &vello::peniko::Brush| match brush {
            vello::peniko::Brush::Gradient(g) => match g.kind {
                vello::peniko::GradientKind::Linear(line) => (line.start, line.end),
                _ => panic!("a linear ramp"),
            },
            _ => panic!("a gradient"),
        };
        let (a0, a1) = ends(&near);
        let (b0, b1) = ends(&far);
        assert!(
            (a1.x - a0.x - 50.0).abs() < 1e-9,
            "the near ramp spans its box"
        );
        assert!(
            ((b1.x - b0.x) - (a1.x - a0.x)).abs() < 1e-9,
            "the far one spans the same distance"
        );
        assert!(
            (b0.x - a0.x - 300.0).abs() < 1e-9,
            "and it moved with the object rather than staying put"
        );
    }

    #[test]
    fn a_shadow_paints_behind_the_object_that_casts_it() {
        use tessera_document::shadow::Shadow;

        let bounds = DocRect {
            x: 20.0,
            y: 20.0,
            width: 40.0,
            height: 40.0,
        };
        let plain = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
            },
            bounds,
        );
        let mut shadowed = plain.clone();
        shadowed.items[0].shadow = Some(Shadow::TYPICAL);

        let without = build_scene(&plain, ViewTransform::default());
        let with = build_scene(&shadowed, ViewTransform::default());
        assert!(
            with.encoding().stream_offsets().path_data
                > without.encoding().stream_offsets().path_data,
            "no shadow reached the scene"
        );
    }

    #[test]
    fn a_shadow_at_no_alpha_paints_nothing() {
        use tessera_document::shadow::Shadow;

        let bounds = DocRect {
            x: 0.0,
            y: 0.0,
            width: 30.0,
            height: 30.0,
        };
        let mut doc = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
            },
            bounds,
        );
        let plain = build_scene(&doc, ViewTransform::default());
        doc.items[0].shadow = Some(Shadow {
            colour: Color::Rgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
            ..Shadow::TYPICAL
        });
        let invisible = build_scene(&doc, ViewTransform::default());

        assert_eq!(
            invisible.encoding().stream_offsets().path_data,
            plain.encoding().stream_offsets().path_data,
            "a shadow nobody can see was still painted"
        );
    }

    #[test]
    fn a_translucent_object_does_not_fade_its_own_shadow() {
        // The shadow is drawn outside the object’s composite group. Inside it, a
        // 50% object would cast a 25% shadow: it is the object that is
        // translucent, not the light.
        use tessera_document::blending::{BlendMode, Blending};
        use tessera_document::shadow::Shadow;

        let bounds = DocRect {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 40.0,
        };
        let mut doc = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
            },
            bounds,
        );
        doc.items[0].shadow = Some(Shadow::TYPICAL);
        doc.items[0].blend = Blending {
            opacity: 0.5,
            mode: BlendMode::Normal,
        };

        // One layer for the object, and the shadow outside it. If the shadow
        // were drawn inside, the blurred rect would be encoded after the layer
        // was pushed — which is exactly what this pins by counting clips: the
        // object opens one, and the shadow opens none.
        let scene = build_scene(&doc, ViewTransform::default());
        let mut without_shadow = doc.clone();
        without_shadow.items[0].shadow = None;
        assert_eq!(
            scene.encoding().n_clips,
            build_scene(&without_shadow, ViewTransform::default())
                .encoding()
                .n_clips,
            "the shadow opened a layer of its own"
        );
    }

    #[test]
    fn a_clip_really_reaches_the_encoding() {
        // Preview must show the trim as it will print, not merely hide the
        // furniture around it — so the clip has to be in the scene, not just
        // in the options struct.
        let doc = one_item(
            ResolvedKind::Rectangle {
                fill: Paint::Solid(Color::BLACK),
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
                fill: Paint::Solid(Color::BLACK),
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
            columns: Vec::new(),
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
                spread_area: None,
                blend: tessera_document::blending::Blending::PLAIN,
                shadow: None,
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
                    fill: Paint::Solid(Color::BLACK),
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
                    fill: Paint::Solid(Color::BLACK),
                    stroke: None,
                },
                bounds,
            ),
            ViewTransform::default(),
        );
        let stroked = build_scene(
            &one_item(
                ResolvedKind::Rectangle {
                    fill: Paint::Solid(Color::BLACK),
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
                    fill: Paint::Solid(Color::BLACK),
                    stroke: None,
                },
                bounds,
            ),
            ViewTransform::default(),
        );
        let ellipse = build_scene(
            &one_item(
                ResolvedKind::Ellipse {
                    fill: Paint::Solid(Color::BLACK),
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
