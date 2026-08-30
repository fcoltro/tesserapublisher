//! End-to-end GPU tests: compile a scene, rasterise it with vello, read the
//! pixels back, and assert on what was actually drawn.
//!
//! These are the only tests that exercise real shaders and a real adapter. They
//! skip rather than fail when no GPU is available, so the suite still passes on
//! headless CI runners without Vulkan/Metal — but when an adapter exists, a
//! regression in the paint or GPU layer shows up here as wrong pixels.

use tessera_renderer::{
    RenderElement, RenderScene, Story, TextAlignment, VelloHeadless, Viewport,
};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;

/// Builds a headless renderer, or returns `None` when the machine has no GPU.
fn renderer() -> Option<VelloHeadless> {
    match VelloHeadless::new(WIDTH, HEIGHT) {
        Ok(renderer) => Some(renderer),
        Err(err) => {
            eprintln!("skipping GPU test: {err}");
            None
        }
    }
}

/// A scene positioned so that document coordinates map 1:1 onto pixels.
fn unit_scene(elements: Vec<RenderElement>) -> RenderScene {
    RenderScene {
        pan_x: 0.0,
        pan_y: 0.0,
        zoom: 1.0,
        elements,
        ..Default::default()
    }
}

/// Reads one pixel as `[r, g, b, a]`.
fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let index = ((y * WIDTH + x) * 4) as usize;
    [
        pixels[index],
        pixels[index + 1],
        pixels[index + 2],
        pixels[index + 3],
    ]
}

#[test]
fn empty_scene_clears_to_the_pasteboard_colour() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    let scene = unit_scene(Vec::new());
    let pixels = renderer
        .render_to_pixels(&scene, Viewport::full(WIDTH as f64, HEIGHT as f64))
        .expect("headless render should succeed");

    assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);

    // The default pasteboard is near-black; assert it is dark rather than an
    // exact triple, so the test does not encode colour-space rounding.
    let [r, g, b, a] = pixel(&pixels, WIDTH / 2, HEIGHT / 2);
    assert!(r < 40 && g < 40 && b < 40, "expected dark pasteboard, got {:?}", [r, g, b]);
    assert_eq!(a, 255, "pasteboard should be opaque");
}

#[test]
fn a_filled_rect_rasterises_where_it_was_placed() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    let scene = unit_scene(vec![RenderElement::RectShape {
        id: 1,
        name: "Red".to_string(),
        x: 16.0,
        y: 16.0,
        width: 32.0,
        height: 32.0,
        rotation: 0.0,
        fill_color: [1.0, 0.0, 0.0, 1.0],
        stroke_color: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
        is_selected: false,
    }]);

    let pixels = renderer
        .render_to_pixels(&scene, Viewport::full(WIDTH as f64, HEIGHT as f64))
        .expect("headless render should succeed");

    // Inside the rect: red dominates.
    let [r, g, b, _] = pixel(&pixels, 32, 32);
    assert!(
        r > 180 && g < 80 && b < 80,
        "expected red inside the rect, got {:?}",
        [r, g, b]
    );

    // Outside the rect: still pasteboard.
    let [r, g, b, _] = pixel(&pixels, 4, 4);
    assert!(
        r < 40 && g < 40 && b < 40,
        "expected pasteboard outside the rect, got {:?}",
        [r, g, b]
    );
}

#[test]
fn the_camera_transform_moves_what_is_drawn() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    let element = RenderElement::RectShape {
        id: 1,
        name: "Block".to_string(),
        x: 0.0,
        y: 0.0,
        width: 16.0,
        height: 16.0,
        rotation: 0.0,
        fill_color: [0.0, 1.0, 0.0, 1.0],
        stroke_color: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
        is_selected: false,
    };

    // Panned by (32, 32), a rect drawn at the document origin must land at
    // pixel (32, 32) and no longer cover the origin.
    let mut scene = unit_scene(vec![element]);
    scene.pan_x = 32.0;
    scene.pan_y = 32.0;

    let pixels = renderer
        .render_to_pixels(&scene, Viewport::full(WIDTH as f64, HEIGHT as f64))
        .expect("headless render should succeed");

    let [_, g, _, _] = pixel(&pixels, 40, 40);
    assert!(g > 180, "expected the panned rect at (40, 40), got g={g}");

    let [_, g, _, _] = pixel(&pixels, 4, 4);
    assert!(g < 80, "origin should be empty after panning, got g={g}");
}

#[test]
fn the_viewport_clips_content_outside_it() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    // A rect covering the whole surface, clipped to the bottom-right quadrant.
    let scene = unit_scene(vec![RenderElement::RectShape {
        id: 1,
        name: "Wash".to_string(),
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 64.0,
        rotation: 0.0,
        fill_color: [1.0, 0.0, 0.0, 1.0],
        stroke_color: None,
        stroke_width: 0.0,
        corner_radius: 0.0,
        is_selected: false,
    }]);

    let viewport = Viewport {
        x: 32.0,
        y: 32.0,
        width: 32.0,
        height: 32.0,
    };
    let pixels = renderer
        .render_to_pixels(&scene, viewport)
        .expect("headless render should succeed");

    // Outside the viewport the rect must not appear, or document content would
    // bleed over the surrounding DOM chrome.
    let [r, _, _, _] = pixel(&pixels, 8, 8);
    assert!(r < 40, "content leaked outside the viewport, got r={r}");
}

#[test]
fn a_closed_path_fills_its_interior() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    // A triangle covering the lower-left half of the surface.
    let scene = unit_scene(vec![RenderElement::PathShape {
        id: 1,
        name: "Triangle".to_string(),
        svg: "M 0 0 L 0 64 L 64 64 Z".to_string(),
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        fill_color: [0.0, 0.0, 1.0, 1.0],
        stroke_color: None,
        stroke_width: 1.0,
        is_closed: true,
        is_selected: false,
    }]);

    let pixels = renderer
        .render_to_pixels(&scene, Viewport::full(WIDTH as f64, HEIGHT as f64))
        .expect("headless render should succeed");

    // Inside the triangle (lower left).
    let [_, _, b, _] = pixel(&pixels, 8, 56);
    assert!(b > 180, "expected the filled triangle, got b={b}");

    // Outside it (upper right), on the far side of the hypotenuse.
    let [_, _, b, _] = pixel(&pixels, 56, 8);
    assert!(b < 80, "triangle should not cover the upper right, got b={b}");
}

#[test]
fn a_path_transform_places_the_outline() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    // The same unit square, translated by (32, 32) through its affine.
    let scene = unit_scene(vec![RenderElement::PathShape {
        id: 1,
        name: "Square".to_string(),
        svg: "M 0 0 L 16 0 L 16 16 L 0 16 Z".to_string(),
        transform: [1.0, 0.0, 0.0, 1.0, 32.0, 32.0],
        fill_color: [0.0, 1.0, 1.0, 1.0],
        stroke_color: None,
        stroke_width: 1.0,
        is_closed: true,
        is_selected: false,
    }]);

    let pixels = renderer
        .render_to_pixels(&scene, Viewport::full(WIDTH as f64, HEIGHT as f64))
        .expect("headless render should succeed");

    let [_, g, _, _] = pixel(&pixels, 40, 40);
    assert!(g > 180, "expected the translated square at (40, 40), got g={g}");

    let [_, g, _, _] = pixel(&pixels, 8, 8);
    assert!(g < 80, "origin should be empty, got g={g}");
}

#[test]
fn an_open_line_is_stroked_not_filled() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    let scene = unit_scene(vec![RenderElement::PathShape {
        id: 1,
        name: "Line".to_string(),
        svg: "M 0 0 L 64 64".to_string(),
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        fill_color: [1.0, 1.0, 0.0, 1.0],
        stroke_color: Some([1.0, 1.0, 0.0, 1.0]),
        stroke_width: 4.0,
        is_closed: false,
        is_selected: false,
    }]);

    let pixels = renderer
        .render_to_pixels(&scene, Viewport::full(WIDTH as f64, HEIGHT as f64))
        .expect("headless render should succeed");

    // On the diagonal the stroke is present.
    let [r, g, _, _] = pixel(&pixels, 32, 32);
    assert!(r > 150 && g > 150, "expected the stroke on the diagonal, got {:?}", [r, g]);

    // Off the diagonal there must be nothing: an open path must not be filled.
    let [r, g, _, _] = pixel(&pixels, 56, 8);
    assert!(r < 80 && g < 80, "open path should not fill, got {:?}", [r, g]);
}

#[test]
fn a_malformed_path_does_not_blank_the_frame() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    // A bad outline alongside a good one: the good one must still draw.
    let scene = unit_scene(vec![
        RenderElement::PathShape {
            id: 1,
            name: "Broken".to_string(),
            svg: "this is not a path".to_string(),
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill_color: [1.0, 0.0, 0.0, 1.0],
            stroke_color: None,
            stroke_width: 1.0,
            is_closed: true,
            is_selected: false,
        },
        RenderElement::RectShape {
            id: 2,
            name: "Good".to_string(),
            x: 16.0,
            y: 16.0,
            width: 32.0,
            height: 32.0,
            rotation: 0.0,
            fill_color: [0.0, 1.0, 0.0, 1.0],
            stroke_color: None,
            stroke_width: 0.0,
            corner_radius: 0.0,
            is_selected: false,
        },
    ]);

    let pixels = renderer
        .render_to_pixels(&scene, Viewport::full(WIDTH as f64, HEIGHT as f64))
        .expect("a malformed path must not fail the render");

    let [_, g, _, _] = pixel(&pixels, 32, 32);
    assert!(g > 180, "the valid shape should still draw, got g={g}");
}

/// Counts pixels that differ noticeably from the dark pasteboard.
fn ink_coverage(pixels: &[u8]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|p| p[0] > 90 || p[1] > 90 || p[2] > 90)
        .count()
}

fn text_block(text: &str, font_size: f32) -> RenderElement {
    RenderElement::TextBlock {
        id: 1,
        name: "Copy".to_string(),
        x: 2.0,
        y: 2.0,
        width: 60.0,
        height: 60.0,
        text: text.to_string(),
        font_size,
        line_height: 1.2,
        align: TextAlignment::Start,
        font_family: None,
        font_weight: 400.0,
        story: None,
        fill_color: [1.0, 1.0, 1.0, 1.0],
        is_selected: false,
    }
}

#[test]
fn text_rasterises_actual_glyphs() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    let blank = renderer
        .render_to_pixels(
            &unit_scene(vec![text_block("", 20.0)]),
            Viewport::full(WIDTH as f64, HEIGHT as f64),
        )
        .expect("render should succeed");
    let blank_ink = ink_coverage(&blank);

    let written = renderer
        .render_to_pixels(
            &unit_scene(vec![text_block("Hello", 20.0)]),
            Viewport::full(WIDTH as f64, HEIGHT as f64),
        )
        .expect("render should succeed");
    let written_ink = ink_coverage(&written);

    // Glyphs must put measurably more ink on the page than an empty frame.
    assert!(
        written_ink > blank_ink + 40,
        "expected glyph coverage; blank={blank_ink}, written={written_ink}"
    );
}

#[test]
fn larger_type_puts_more_ink_on_the_page() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    let small = renderer
        .render_to_pixels(
            &unit_scene(vec![text_block("AAA", 8.0)]),
            Viewport::full(WIDTH as f64, HEIGHT as f64),
        )
        .expect("render should succeed");
    let large = renderer
        .render_to_pixels(
            &unit_scene(vec![text_block("AAA", 24.0)]),
            Viewport::full(WIDTH as f64, HEIGHT as f64),
        )
        .expect("render should succeed");

    assert!(
        ink_coverage(&large) > ink_coverage(&small),
        "larger type should cover more pixels"
    );
}

#[test]
fn overset_text_is_marked_on_the_canvas() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    // Far more text than a 60x60 frame can hold.
    let mut element = text_block(&"overflowing text ".repeat(40), 14.0);
    if let RenderElement::TextBlock { height, .. } = &mut element {
        *height = 20.0;
    }

    let pixels = renderer
        .render_to_pixels(
            &unit_scene(vec![element]),
            Viewport::full(WIDTH as f64, HEIGHT as f64),
        )
        .expect("render should succeed");

    // The overset marker is a red square at the frame's bottom-right corner
    // (frame spans x 2..62, y 2..22, so the marker covers x 52..62, y 12..22).
    let [r, g, b, _] = pixel(&pixels, 58, 18);
    assert!(
        r > 150 && g < 110 && b < 110,
        "expected the red overset marker, got {:?}",
        [r, g, b]
    );
}


/// Builds a two-frame threaded story laid side by side.
fn threaded_scene(story_text: &str) -> RenderScene {
    let frame = |id: u32, x: f32, index: u32| RenderElement::TextBlock {
        id,
        name: format!("Frame {id}"),
        x,
        y: 2.0,
        width: 28.0,
        height: 30.0,
        text: String::new(),
        font_size: 8.0,
        line_height: 1.1,
        align: TextAlignment::Start,
        font_family: None,
        font_weight: 400.0,
        story: Some([1, index]),
        fill_color: [1.0, 1.0, 1.0, 1.0],
        is_selected: false,
    };

    let mut scene = unit_scene(vec![frame(1, 2.0, 0), frame(2, 34.0, 1)]);
    scene.stories = vec![Story {
        id: 1,
        text: story_text.to_string(),
        frames: vec![[28.0, 30.0], [28.0, 30.0]],
    }];
    scene
}

#[test]
fn a_threaded_story_paints_into_both_frames() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    // Enough text that the first frame cannot hold it all.
    let pixels = renderer
        .render_to_pixels(
            &threaded_scene("alpha beta gamma delta epsilon zeta eta theta iota kappa"),
            Viewport::full(WIDTH as f64, HEIGHT as f64),
        )
        .expect("render should succeed");

    // Count ink in each frame's half of the surface.
    let ink_in = |x0: u32, x1: u32| {
        let mut count = 0;
        for y in 0..HEIGHT {
            for x in x0..x1 {
                let [r, g, b, _] = pixel(&pixels, x, y);
                if r > 90 || g > 90 || b > 90 {
                    count += 1;
                }
            }
        }
        count
    };

    assert!(ink_in(2, 30) > 20, "the first frame should carry text");
    assert!(
        ink_in(34, 62) > 20,
        "the overflow must continue into the second frame"
    );
}

#[test]
fn a_short_story_leaves_the_second_frame_empty() {
    let Some(mut renderer) = renderer() else {
        return;
    };

    let pixels = renderer
        .render_to_pixels(
            &threaded_scene("hi"),
            Viewport::full(WIDTH as f64, HEIGHT as f64),
        )
        .expect("render should succeed");

    let mut second_frame_ink = 0;
    for y in 0..HEIGHT {
        for x in 34..62 {
            let [r, g, b, _] = pixel(&pixels, x, y);
            if r > 90 || g > 90 || b > 90 {
                second_frame_ink += 1;
            }
        }
    }

    assert!(
        second_frame_ink < 10,
        "a story that fits frame one must not spill, got {second_frame_ink}"
    );
}
