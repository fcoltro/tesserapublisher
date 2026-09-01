//! GPU-backed rendering tests.
//!
//! RUN ALONE, IN THE FOREGROUND:
//!
//! ```text
//! cargo test -p tessera_render --test gpu_render
//! ```
//!
//! Never inside `cargo test --workspace`: two GPU test binaries contending
//! for the same adapter deadlock, and adapter acquisition hangs
//! intermittently on this hardware regardless. A hang looks exactly like a
//! slow compile — if there is no output for two minutes, kill the
//! `gpu_render-*` binary and any `cargo.exe`, then retry once.

use tessera_color::Color;
use tessera_document::ids::FrameId;
use tessera_geometry::{DocPoint, DocRect, ViewTransform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};
use tessera_render::headless::HeadlessRenderer;
use tessera_render::scene::build_scene;

const W: u32 = 100;
const H: u32 = 100;

fn page() -> DocRect {
    DocRect {
        x: 0.0,
        y: 0.0,
        width: f64::from(W),
        height: f64::from(H),
    }
}

fn rect_doc(bounds: DocRect, fill: Color) -> ResolvedDocument {
    ResolvedDocument {
        items: vec![ResolvedItem {
            frame: FrameId::default(),
            bounds,
            kind: ResolvedKind::Rectangle { fill, stroke: None },
        }],
    }
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * W as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

#[test]
fn an_empty_page_renders_white() {
    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let scene = build_scene(
        &ResolvedDocument::default(),
        ViewTransform::default(),
        page(),
    );
    let pixels = renderer.render(&scene).expect("render");

    assert_eq!(pixels.len(), (W * H * 4) as usize, "tightly packed RGBA8");
    assert_eq!(pixel(&pixels, 50, 50), [255, 255, 255], "the page is white");
}

#[test]
fn a_black_rectangle_renders_black_where_it_sits_and_nowhere_else() {
    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let scene = build_scene(
        &rect_doc(
            DocRect {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 50.0,
            },
            Color::BLACK,
        ),
        ViewTransform::default(),
        page(),
    );
    let pixels = renderer.render(&scene).expect("render");

    assert_eq!(pixel(&pixels, 30, 30), [0, 0, 0], "inside the rectangle");
    assert_eq!(
        pixel(&pixels, 90, 90),
        [255, 255, 255],
        "outside the rectangle"
    );
}

/// The row-stride test. 100px * 4 bytes = 400, padded to a 512-byte row, so
/// 112 bytes per row must be skipped on read-back. If they are not, the image
/// shears progressively and a rectangle's lower rows land at the wrong x.
#[test]
fn read_back_drops_row_padding_rather_than_shearing_the_image() {
    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let scene = build_scene(
        &rect_doc(
            DocRect {
                x: 10.0,
                y: 0.0,
                width: 20.0,
                height: 100.0,
            },
            Color::BLACK,
        ),
        ViewTransform::default(),
        page(),
    );
    let pixels = renderer.render(&scene).expect("render");

    // A vertical bar: the same columns must be black on every row, top to
    // bottom. Shearing shows up as the bar drifting sideways down the image.
    for y in [0usize, 25, 50, 75, 99] {
        assert_eq!(pixel(&pixels, 20, y), [0, 0, 0], "bar interior at row {y}");
        assert_eq!(
            pixel(&pixels, 60, y),
            [255, 255, 255],
            "clear of the bar at row {y}"
        );
    }
}

#[test]
fn the_camera_transform_moves_what_is_rendered() {
    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let doc = rect_doc(
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 20.0,
        },
        Color::BLACK,
    );

    let at_origin = renderer
        .render(&build_scene(&doc, ViewTransform::default(), page()))
        .expect("render");
    assert_eq!(pixel(&at_origin, 10, 10), [0, 0, 0]);

    // Pan the camera 50pt left, so the rectangle moves right on screen.
    let panned = ViewTransform {
        pan: DocPoint { x: -50.0, y: 0.0 },
        zoom: 1.0,
    };
    let after = renderer
        .render(&build_scene(&doc, panned, page()))
        .expect("render");
    assert_eq!(pixel(&after, 10, 10), [255, 255, 255], "moved away");
    assert_eq!(pixel(&after, 60, 10), [0, 0, 0], "moved here");
}

#[test]
fn text_puts_dark_pixels_on_the_page() {
    let mut shaper = tessera_text::shape::Shaper::new();
    let mut story = tessera_text::story::Story::new("HHHH");
    story.style.size = 48.0;
    let shaped = shaper.shape(&story, 400.0);
    assert!(shaped.glyph_count() > 0, "the fixture must actually shape");

    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let scene = build_scene(
        &ResolvedDocument {
            items: vec![ResolvedItem {
                frame: FrameId::default(),
                bounds: DocRect {
                    x: 2.0,
                    y: 2.0,
                    width: 400.0,
                    height: 60.0,
                },
                kind: ResolvedKind::Text {
                    shaped,
                    color: Color::BLACK,
                },
            }],
        },
        ViewTransform::default(),
        page(),
    );
    let pixels = renderer.render(&scene).expect("render");

    let (rgba, _) = pixels.as_chunks::<4>();
    let dark = rgba
        .iter()
        .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
        .count();
    assert!(dark > 20, "glyphs must actually mark the page, saw {dark}");
}
