//! Pure translation from a backend-agnostic [`RenderScene`] into a [`vello::Scene`].
//!
//! This module contains no GPU types on purpose. Everything here is a pure
//! function of the compiled scene description, which means it can be unit
//! tested without a wgpu device and reused unchanged by any presentation
//! target (a window surface, an offscreen texture, or a future WASM host).

use vello::kurbo::{Affine, BezPath, Ellipse, Line, Rect, RoundedRect, Stroke};
use vello::peniko::{Color, Fill, ImageBrush};
use vello::Scene;

use crate::images::ImageCache;
use crate::scene::{RenderElement, RenderScene};
use crate::text::{TextEngine, TextStyle, ThreadSlice};
use std::collections::HashMap;
use tessera_core::BaselineGrid;

/// The sub-rectangle of the render surface that the document is drawn into.
///
/// The GPU surface spans the whole window, while the DOM reserves only part of
/// it for the canvas. The viewport carries that reserved region, in physical
/// pixels, so document content is offset and clipped to match the layout the
/// user actually sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Viewport {
    /// A viewport covering an entire surface of the given size.
    pub fn full(width: f64, height: f64) -> Self {
        Self { x: 0.0, y: 0.0, width, height }
    }

    /// The viewport as a rectangle in surface space.
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.x + self.width, self.y + self.height)
    }
}

/// Width of the selection outline, in screen pixels.
const SELECTION_STROKE_PX: f64 = 1.5;
/// Half-extent of a selection corner handle, in screen pixels.
const SELECTION_HANDLE_PX: f64 = 4.0;

/// Converts a `[r, g, b, a]` component array (each in `0.0..=1.0`) into a color.
pub fn color_from_rgba(rgba: [f32; 4]) -> Color {
    Color::new(rgba)
}

/// The root transform mapping document space onto screen space.
///
/// Because the camera is a single affine applied to the whole scene, panning and
/// zooming never require recompiling the ECS into a new [`RenderScene`].
pub fn camera_affine(pan_x: f32, pan_y: f32, zoom: f32) -> Affine {
    Affine::translate((pan_x as f64, pan_y as f64)) * Affine::scale(zoom as f64)
}

/// The type settings of the frame that starts a story.
///
/// Every frame in a chain shares the head's settings, so the story reflows
/// consistently rather than changing size partway through.
fn story_style(render_scene: &RenderScene, story_id: u32) -> TextStyle {
    render_scene
        .elements
        .iter()
        .find_map(|element| match element {
            RenderElement::TextBlock {
                id,
                font_size,
                line_height,
                align,
                font_family,
                font_weight,
                tracking,
                baseline,
                ..
            } if *id == story_id => Some(TextStyle {
                font_size: *font_size,
                line_height: *line_height,
                align: *align,
                font_family: font_family.clone(),
                font_weight: *font_weight,
                tracking: *tracking,
                // The head's grid governs the whole chain: a story must not
                // change its rhythm partway through.
                baseline_increment: baseline.map(|[increment, _]| increment),
            }),
            _ => None,
        })
        .unwrap_or_default()
}

/// Composes a rotation about an element's own centre onto `base`.
///
/// `rotation` is in radians, matching `Transform::rotation` in `tessera-core`,
/// which feeds it to `f32::cos`/`f32::sin` when recomputing bounding boxes.
fn rotated_about(base: Affine, rotation: f32, cx: f64, cy: f64) -> Affine {
    if rotation == 0.0 {
        return base;
    }
    base * Affine::translate((cx, cy))
        * Affine::rotate(rotation as f64)
        * Affine::translate((-cx, -cy))
}

/// Turns compiled scenes into vello draw commands.
///
/// The painter owns the text engine because shaping needs the system font
/// database, which is expensive to build and must be reused across frames.
#[derive(Default)]
pub struct Painter {
    text: TextEngine,
    /// Decoded image proxies. Owned here for the same reason as the font
    /// database: decoding is far too expensive to repeat every frame.
    images: ImageCache,
}

impl Painter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Paints a compiled [`RenderScene`] into a fresh [`vello::Scene`].
    ///
    /// The document occupies the whole surface. Use [`Painter::paint_into`] to
    /// place it in a sub-rectangle.
    pub fn paint(&mut self, render_scene: &RenderScene) -> Scene {
        let mut scene = Scene::new();
        self.paint_into(&mut scene, render_scene, Viewport::full(0.0, 0.0));
        scene
    }

    /// Access to the text engine, for measuring without painting.
    pub fn text_engine(&mut self) -> &mut TextEngine {
        &mut self.text
    }

    /// Access to the image cache, for link status without painting.
    pub fn image_cache(&mut self) -> &mut ImageCache {
        &mut self.images
    }

/// Paints into an existing scene, resetting it first.
///
/// Reusing one `Scene` across frames avoids reallocating its encoding buffers.
pub fn paint_into(&mut self, scene: &mut Scene, render_scene: &RenderScene, viewport: Viewport) {
    scene.reset();

    // Everything is clipped to the viewport so document content never bleeds
    // over the surrounding DOM chrome.
    let clipped = viewport.width > 0.0 && viewport.height > 0.0;
    if clipped {
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &viewport.rect());
    }

    // Each story is flowed once, not once per frame: a chain of N frames would
    // otherwise cost N shaping passes over the whole story.
    let mut flows: HashMap<u32, Vec<ThreadSlice>> = HashMap::new();
    for story in &render_scene.stories {
        let style = story_style(render_scene, story.id);
        let frames: Vec<(f32, f32)> = story.frames.iter().map(|f| (f[0], f[1])).collect();
        flows.insert(story.id, self.text.flow(&story.text, &style, &frames));
    }

    let camera = Affine::translate((viewport.x, viewport.y))
        * camera_affine(render_scene.pan_x, render_scene.pan_y, render_scene.zoom);
    // Selection chrome should stay a constant thickness on screen regardless of
    // zoom, so its document-space size divides the zoom factor back out.
    let zoom = render_scene.zoom.max(f32::EPSILON) as f64;

    for element in &render_scene.elements {
        match element {
            RenderElement::PageSurface {
                x,
                y,
                width,
                height,
                shadow_blur,
                ..
            } => {
                let rect = Rect::new(
                    *x as f64,
                    *y as f64,
                    (*x + *width) as f64,
                    (*y + *height) as f64,
                );

                // Drop shadow first, so the paper surface lands on top of it.
                scene.draw_blurred_rounded_rect(
                    camera,
                    rect,
                    Color::new([0.0, 0.0, 0.0, 0.55]),
                    2.0,
                    (*shadow_blur as f64).max(0.1),
                );
                scene.fill(
                    Fill::NonZero,
                    camera,
                    Color::new([1.0, 1.0, 1.0, 1.0]),
                    None,
                    &rect,
                );
            }

            RenderElement::GuideLine {
                is_vertical,
                position,
                is_active,
                ..
            } => {
                // A snapped guide brightens so the user can see what caught it.
                let color = if *is_active {
                    Color::new([0.2, 0.95, 0.75, 1.0])
                } else {
                    Color::new([0.25, 0.75, 0.95, 0.7])
                };
                let width = if *is_active { 2.0 } else { 1.0 } / zoom;

                // Guides run the full extent of the visible document area. The
                // viewport clip keeps this from painting over the DOM chrome.
                let span = 100_000.0;
                let line = if *is_vertical {
                    Line::new((*position as f64, -span), (*position as f64, span))
                } else {
                    Line::new((-span, *position as f64), (span, *position as f64))
                };
                scene.stroke(&Stroke::new(width), camera, color, None, &line);
            }

            RenderElement::PageChrome {
                bleed,
                slug,
                margins,
                columns,
                column_top,
                column_bottom,
                baseline_grid,
                ..
            } => {
                // Guide colours follow the conventions of print layout tools so
                // the meaning of each line is recognisable at a glance.
                let bleed_color = Color::new([0.85, 0.25, 0.25, 0.9]);
                let slug_color = Color::new([0.35, 0.55, 0.9, 0.9]);
                let margin_color = Color::new([0.95, 0.3, 0.75, 0.9]);
                let column_color = Color::new([0.65, 0.45, 0.95, 0.75]);

                // Guides are drawn a constant width on screen regardless of zoom.
                let hairline = Stroke::new(1.0 / zoom);

                if slug != bleed {
                    scene.stroke(
                        &hairline,
                        camera,
                        slug_color,
                        None,
                        &Rect::new(slug[0] as f64, slug[1] as f64, slug[2] as f64, slug[3] as f64),
                    );
                }
                scene.stroke(
                    &hairline,
                    camera,
                    bleed_color,
                    None,
                    &Rect::new(bleed[0] as f64, bleed[1] as f64, bleed[2] as f64, bleed[3] as f64),
                );
                scene.stroke(
                    &hairline,
                    camera,
                    margin_color,
                    None,
                    &Rect::new(
                        margins[0] as f64,
                        margins[1] as f64,
                        margins[2] as f64,
                        margins[3] as f64,
                    ),
                );

                // Only the gutters between columns need drawing: the outer edges
                // are already the margin box.
                for (index, column) in columns.iter().enumerate() {
                    if index > 0 {
                        scene.stroke(
                            &hairline,
                            camera,
                            column_color,
                            None,
                            &Line::new(
                                (column[0] as f64, *column_top as f64),
                                (column[0] as f64, *column_bottom as f64),
                            ),
                        );
                    }
                    if index + 1 < columns.len() {
                        scene.stroke(
                            &hairline,
                            camera,
                            column_color,
                            None,
                            &Line::new(
                                (column[1] as f64, *column_top as f64),
                                (column[1] as f64, *column_bottom as f64),
                            ),
                        );
                    }
                }

                // The baseline grid is drawn last and faintest of the page
                // chrome: it is the densest set of lines on the page, so it has
                // to sit behind the margins and columns visually rather than
                // competing with them.
                if let Some([origin, increment]) = baseline_grid {
                    let baseline_color = Color::new([0.4, 0.7, 0.85, 0.35]);
                    let grid = BaselineGrid { increment: *increment, start: 0.0, visible: true };

                    for y in grid.lines_between(*origin, *column_top, *column_bottom) {
                        scene.stroke(
                            &hairline,
                            camera,
                            baseline_color,
                            None,
                            &Line::new(
                                (margins[0] as f64, y as f64),
                                (margins[2] as f64, y as f64),
                            ),
                        );
                    }
                }
            }

            RenderElement::ImageFrame {
                x,
                y,
                width,
                height,
                rotation,
                path,
                fit,
                ..
            } => {
                let box_rect =
                    Rect::new(*x as f64, *y as f64, (*x + *width) as f64, (*y + *height) as f64);
                let transform = rotated_about(
                    camera,
                    *rotation,
                    (*x + *width / 2.0) as f64,
                    (*y + *height / 2.0) as f64,
                );

                match self.images.get(path) {
                    Some(image) => {
                        let (iw, ih) = (image.width as f64, image.height as f64);
                        let (bw, bh) = (*width as f64, *height as f64);

                        // Fill covers the box and crops; Fit shows everything
                        // and leaves margin; Stretch abandons the aspect ratio.
                        let (sx, sy) = if iw <= 0.0 || ih <= 0.0 {
                            (1.0, 1.0)
                        } else {
                            match fit {
                                tessera_core::ImageFit::Stretch => (bw / iw, bh / ih),
                                tessera_core::ImageFit::Fit => {
                                    let s = (bw / iw).min(bh / ih);
                                    (s, s)
                                }
                                tessera_core::ImageFit::Fill => {
                                    let s = (bw / iw).max(bh / ih);
                                    (s, s)
                                }
                            }
                        };

                        // Centred in the box, so a crop takes from both edges
                        // rather than always from the right and bottom.
                        let tx = *x as f64 + (bw - iw * sx) / 2.0;
                        let ty = *y as f64 + (bh - ih * sy) / 2.0;

                        // The frame is the crop the designer drew, so anything
                        // outside it must not paint.
                        scene.push_clip_layer(Fill::NonZero, transform, &box_rect);
                        let brush = ImageBrush::new((*image).clone());
                        scene.draw_image(
                            brush.as_ref(),
                            transform
                                * Affine::translate((tx, ty))
                                * Affine::scale_non_uniform(sx, sy),
                        );
                        scene.pop_layer();
                    }
                    None => {
                        // A broken link is drawn, not skipped: a silently empty
                        // frame is how a missing photograph reaches print.
                        scene.fill(
                            Fill::NonZero,
                            transform,
                            Color::from_rgba8(40, 26, 30, 255),
                            None,
                            &box_rect,
                        );
                        scene.stroke(
                            &Stroke::new(1.5),
                            transform,
                            Color::from_rgba8(248, 113, 113, 220),
                            None,
                            &box_rect,
                        );
                        // Crossed diagonals, the universal "no image here".
                        for line in [
                            Line::new(
                                (box_rect.x0, box_rect.y0),
                                (box_rect.x1, box_rect.y1),
                            ),
                            Line::new(
                                (box_rect.x1, box_rect.y0),
                                (box_rect.x0, box_rect.y1),
                            ),
                        ] {
                            scene.stroke(
                                &Stroke::new(1.0),
                                transform,
                                Color::from_rgba8(248, 113, 113, 160),
                                None,
                                &line,
                            );
                        }
                    }
                }
            }

            RenderElement::RectShape {
                x,
                y,
                width,
                height,
                rotation,
                fill_color,
                stroke_color,
                stroke_width,
                corner_radius,
                ..
            } => {
                let rect = RoundedRect::new(
                    *x as f64,
                    *y as f64,
                    (*x + *width) as f64,
                    (*y + *height) as f64,
                    *corner_radius as f64,
                );
                let transform = rotated_about(
                    camera,
                    *rotation,
                    (*x + *width / 2.0) as f64,
                    (*y + *height / 2.0) as f64,
                );

                scene.fill(
                    Fill::NonZero,
                    transform,
                    color_from_rgba(*fill_color),
                    None,
                    &rect,
                );
                if let Some(stroke) = stroke_color {
                    scene.stroke(
                        &Stroke::new(*stroke_width as f64),
                        transform,
                        color_from_rgba(*stroke),
                        None,
                        &rect,
                    );
                }
            }

            RenderElement::EllipseShape {
                cx,
                cy,
                rx,
                ry,
                rotation,
                fill_color,
                stroke_color,
                stroke_width,
                ..
            } => {
                let ellipse = Ellipse::new(
                    (*cx as f64, *cy as f64),
                    (*rx as f64, *ry as f64),
                    *rotation as f64,
                );

                scene.fill(
                    Fill::NonZero,
                    camera,
                    color_from_rgba(*fill_color),
                    None,
                    &ellipse,
                );
                if let Some(stroke) = stroke_color {
                    scene.stroke(
                        &Stroke::new(*stroke_width as f64),
                        camera,
                        color_from_rgba(*stroke),
                        None,
                        &ellipse,
                    );
                }
            }

            RenderElement::TextBlock {
                x,
                y,
                width,
                height,
                text,
                font_size,
                line_height,
                align,
                font_family,
                font_weight,
                tracking,
                story,
                baseline,
                fill_color,
                is_selected,
                ..
            } => {
                let style = TextStyle {
                    font_size: *font_size,
                    line_height: *line_height,
                    align: *align,
                    font_family: font_family.clone(),
                    font_weight: *font_weight,
                    tracking: *tracking,
                    baseline_increment: baseline.map(|[increment, _]| increment),
                };

                // A threaded frame shows only its slice of the story; an
                // unthreaded one shows its own text.
                let (body, threaded_overset) = match story.and_then(|[story_id, index]| {
                    let slices = flows.get(&story_id)?;
                    let story = render_scene.stories.iter().find(|s| s.id == story_id)?;
                    let slice = slices.get(index as usize)?;
                    Some((slice.text(&story.text).to_string(), slice.is_overset))
                }) {
                    Some((body, overset)) => (body, overset),
                    None => (text.clone(), false),
                };

                let shaped = self.text.shape(&body, &style, *width, *height);
                let is_overset = shaped.is_overset || threaded_overset;

                // Snapped leading keeps lines a whole increment apart, but the
                // block as a whole still has to be nudged so its first baseline
                // lands on a rung rather than merely being correctly spaced.
                let grid_shift = baseline
                    .map(|[increment, origin]| {
                        let grid = BaselineGrid { increment, start: 0.0, visible: false };
                        grid.shift_onto_grid(origin, *y + shaped.first_baseline)
                    })
                    .unwrap_or(0.0);

                // Text is laid out in the frame's local space, so the glyph
                // transform translates local origin to the frame's corner.
                let placement =
                    camera * Affine::translate((*x as f64, (*y + grid_shift) as f64));
                self.text
                    .draw(scene, &shaped, placement, color_from_rgba(*fill_color));

                // Overset text gets the red marker layout tools use, so the
                // condition is visible on the canvas rather than only in a panel.
                if is_overset {
                    let marker = Rect::new(
                        (*x + *width) as f64 - 10.0,
                        (*y + *height) as f64 - 10.0,
                        (*x + *width) as f64,
                        (*y + *height) as f64,
                    );
                    scene.fill(
                        Fill::NonZero,
                        camera,
                        Color::new([0.9, 0.2, 0.2, 1.0]),
                        None,
                        &marker,
                    );
                }

                // An empty frame would otherwise be invisible and unclickable.
                if body.is_empty() || *is_selected {
                    let rect = Rect::new(
                        *x as f64,
                        *y as f64,
                        (*x + *width) as f64,
                        (*y + *height) as f64,
                    );
                    let mut hint = *fill_color;
                    hint[3] *= 0.35;
                    scene.stroke(
                        &Stroke::new(1.0 / zoom),
                        camera,
                        color_from_rgba(hint),
                        None,
                        &rect,
                    );
                }
            }

            RenderElement::PathShape {
                svg,
                transform,
                fill_color,
                stroke_color,
                stroke_width,
                is_closed,
                ..
            } => {
                // A malformed outline is skipped rather than aborting the frame,
                // so one bad path cannot blank the whole document.
                let Ok(outline) = BezPath::from_svg(svg) else {
                    continue;
                };
                let placement = camera
                    * Affine::new([
                        transform[0] as f64,
                        transform[1] as f64,
                        transform[2] as f64,
                        transform[3] as f64,
                        transform[4] as f64,
                        transform[5] as f64,
                    ]);

                if *is_closed {
                    scene.fill(
                        Fill::NonZero,
                        placement,
                        color_from_rgba(*fill_color),
                        None,
                        &outline,
                    );
                }
                if let Some(stroke) = stroke_color {
                    scene.stroke(
                        &Stroke::new(*stroke_width as f64),
                        placement,
                        color_from_rgba(*stroke),
                        None,
                        &outline,
                    );
                } else if !*is_closed {
                    // An open outline with no stroke colour would be invisible.
                    scene.stroke(
                        &Stroke::new(*stroke_width as f64),
                        placement,
                        color_from_rgba(*fill_color),
                        None,
                        &outline,
                    );
                }
            }

            RenderElement::SelectionOverlay {
                min_x,
                min_y,
                max_x,
                max_y,
                corner_nodes,
                ..
            } => {
                let accent = Color::new([0.35, 0.70, 1.0, 1.0]);
                let bounds = Rect::new(*min_x as f64, *min_y as f64, *max_x as f64, *max_y as f64);

                scene.stroke(
                    &Stroke::new(SELECTION_STROKE_PX / zoom),
                    camera,
                    accent,
                    None,
                    &bounds,
                );

                let handle = SELECTION_HANDLE_PX / zoom;
                for node in corner_nodes {
                    let (nx, ny) = (node[0] as f64, node[1] as f64);
                    let square = Rect::new(nx - handle, ny - handle, nx + handle, ny + handle);
                    scene.fill(
                        Fill::NonZero,
                        camera,
                        Color::new([1.0, 1.0, 1.0, 1.0]),
                        None,
                        &square,
                    );
                    scene.stroke(
                        &Stroke::new(SELECTION_STROKE_PX / zoom),
                        camera,
                        accent,
                        None,
                        &square,
                    );
                }
            }
        }
    }

    if clipped {
        scene.pop_layer();
    }
}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::RenderScene;
    use vello::kurbo::Point;

    #[test]
    fn camera_affine_matches_core_screen_mapping() {
        // Must agree with Camera::document_to_screen in tessera-core, otherwise
        // clicks would select a different entity than the one drawn under them.
        let camera = tessera_core::Camera {
            pan_x: 100.0,
            pan_y: 50.0,
            zoom: 2.0,
            viewport_width: 1200.0,
            viewport_height: 800.0,
        };
        let (expected_x, expected_y) = camera.document_to_screen(50.0, 50.0);

        let affine = camera_affine(camera.pan_x, camera.pan_y, camera.zoom);
        let mapped = affine * Point::new(50.0, 50.0);

        assert!((mapped.x - expected_x as f64).abs() < 1e-6);
        assert!((mapped.y - expected_y as f64).abs() < 1e-6);
    }

    #[test]
    fn camera_affine_inverts_to_document_space() {
        let affine = camera_affine(37.5, -12.0, 0.75);
        let round_tripped = affine.inverse() * (affine * Point::new(210.0, 297.0));

        assert!((round_tripped.x - 210.0).abs() < 1e-9);
        assert!((round_tripped.y - 297.0).abs() < 1e-9);
    }

    #[test]
    fn color_conversion_preserves_components() {
        let color = color_from_rgba([0.25, 0.5, 0.75, 0.5]);
        assert_eq!(color.components, [0.25, 0.5, 0.75, 0.5]);
    }

    #[test]
    fn rotation_about_centre_leaves_centre_fixed() {
        let base = camera_affine(0.0, 0.0, 1.0);
        let transform = rotated_about(base, std::f32::consts::FRAC_PI_2, 100.0, 100.0);
        let centre = transform * Point::new(100.0, 100.0);

        assert!((centre.x - 100.0).abs() < 1e-9);
        assert!((centre.y - 100.0).abs() < 1e-9);
    }

    #[test]
    fn zero_rotation_is_the_identity_composition() {
        let base = camera_affine(10.0, 20.0, 1.5);
        let transform = rotated_about(base, 0.0, 50.0, 50.0);

        assert_eq!(base.as_coeffs(), transform.as_coeffs());
    }

    #[test]
    fn painting_an_empty_scene_produces_no_layers() {
        let painted = Painter::new().paint(&RenderScene::default());
        assert_eq!(painted.encoding().n_paths, 0);
    }

    #[test]
    fn painting_encodes_every_element() {
        let render_scene = RenderScene {
            elements: vec![
            RenderElement::PageSurface {
                page_number: 1,
                x: 0.0,
                y: 0.0,
                width: 600.0,
                height: 800.0,
                bleed: 3.0,
                shadow_blur: 15.0,
            },
            RenderElement::RectShape {
                id: 1,
                name: "Rect".to_string(),
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 50.0,
                rotation: 0.0,
                fill_color: [1.0, 0.0, 0.0, 1.0],
                stroke_color: Some([0.0, 0.0, 0.0, 1.0]),
                stroke_width: 2.0,
                corner_radius: 4.0,
                is_selected: false,
            },
            ],
            ..Default::default()
        };

        let painted = Painter::new().paint(&render_scene);
        assert!(painted.encoding().n_paths > 0);
    }

    #[test]
    fn repainting_resets_rather_than_accumulates() {
        let render_scene = RenderScene {
            elements: vec![RenderElement::RectShape {
            id: 1,
            name: "Rect".to_string(),
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            rotation: 0.0,
            fill_color: [1.0, 1.0, 1.0, 1.0],
            stroke_color: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
            is_selected: false,
            }],
            ..Default::default()
        };

        let mut painter = Painter::new();
        let mut scene = Scene::new();
        painter.paint_into(&mut scene, &render_scene, Viewport::full(800.0, 600.0));
        let after_first = scene.encoding().n_paths;
        painter.paint_into(&mut scene, &render_scene, Viewport::full(800.0, 600.0));

        assert_eq!(after_first, scene.encoding().n_paths);
    }
}
