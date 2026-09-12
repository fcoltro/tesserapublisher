//! GPU-backed rendering tests.
//!
//! RUN ALONE, IN THE FOREGROUND:
//!
//! ```text
//! cargo test -p tessera_render --test gpu_render -- --ignored
//! ```
//!
//! Every test here is `#[ignore]`d, which is what keeps them out of
//! `cargo test --workspace`. That is deliberate and load-bearing for two
//! reasons: two GPU test binaries contending for the same adapter deadlock,
//! and CI runners have no GPU adapter at all. An earlier CI config claimed
//! `--lib --tests` excluded them — it does not, `--tests` includes
//! integration tests — so the exclusion now lives in the tests themselves
//! where it cannot be forgotten.
//!
//! A hang looks exactly like a slow compile: if there is no output for two
//! minutes, kill the `gpu_render-*` binary and any `cargo.exe`, then retry
//! once.

use tessera_color::Color;
use tessera_document::ids::FrameId;
use tessera_geometry::{DocPoint, DocRect, Transform, ViewTransform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};
use tessera_render::headless::HeadlessRenderer;
use tessera_render::scene::build_scene;
use tessera_text::story::NoStyles;

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

/// The test page, resolved with no margins, bleed or slug — so nothing
/// non-printing is drawn and every asserted pixel belongs to the content.
fn resolved_page() -> tessera_layout::ResolvedPage {
    tessera_layout::ResolvedPage {
        bounds: page(),
        margins: page(),
        bleed: page(),
        slug: page(),
        columns: Vec::new(),
    }
}

fn empty_doc() -> ResolvedDocument {
    ResolvedDocument {
        items: Vec::new(),
        pages: vec![resolved_page()],
    }
}

fn rect_doc(bounds: DocRect, fill: Color) -> ResolvedDocument {
    ResolvedDocument {
        items: vec![ResolvedItem {
            frame: FrameId::default(),
            bounds,
            transform: Transform::IDENTITY,
            spread_area: None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            kind: ResolvedKind::Rectangle {
                outline: None,
                fill: tessera_document::paint::Paint::Solid(fill),
                stroke: None,
            },
        }],
        pages: vec![resolved_page()],
    }
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 3] {
    let i = (y * W as usize + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

#[test]
#[ignore = "needs a GPU adapter; run with -- --ignored"]
fn printing_clips_keep_all_pages_and_hide_artwork_between_spreads() {
    use tessera_render::scene::{SceneOptions, build_scene_with};
    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let mut doc = rect_doc(page(), Color::BLACK);
    doc.pages = [(10.0, 10.0), (50.0, 10.0), (10.0, 55.0)]
        .into_iter()
        .map(|(x, y)| {
            let bounds = DocRect {
                x,
                y,
                width: 25.0,
                height: 25.0,
            };
            let grow = |by| DocRect {
                x: x - by,
                y: y - by,
                width: 25.0 + by * 2.0,
                height: 25.0 + by * 2.0,
            };
            tessera_layout::ResolvedPage {
                bounds,
                margins: bounds,
                bleed: grow(3.0),
                slug: grow(6.0),
                columns: vec![],
            }
        })
        .collect();
    for mode in 0..3 {
        let clip = doc
            .pages
            .iter()
            .map(|p| match mode {
                0 => p.bounds,
                1 => p.bleed,
                _ => p.slug,
            })
            .collect();
        let options = SceneOptions {
            rules: false,
            clip: Some(clip),
        };
        let scene = build_scene_with(&doc, ViewTransform::default(), options.clone());
        let pixels = renderer.render(&scene).expect("render");
        for (x, y) in [(20, 20), (60, 20), (20, 65)] {
            assert_eq!(
                pixel(&pixels, x, y),
                [0, 0, 0],
                "page missing in mode {mode}"
            );
        }
        assert_eq!(
            pixel(&pixels, 42, 20),
            [255; 3],
            "horizontal pasteboard leaked"
        );
        assert_eq!(
            pixel(&pixels, 20, 45),
            [255; 3],
            "inter-spread pasteboard leaked"
        );
        assert_eq!(
            pixel(&pixels, 8, 20),
            if mode >= 1 { [0; 3] } else { [255; 3] }
        );
        assert_eq!(
            pixel(&pixels, 5, 20),
            if mode == 2 { [0; 3] } else { [255; 3] }
        );

        // Paper must extend to the revealed bleed/slug even without artwork.
        let blank = ResolvedDocument {
            items: vec![],
            pages: doc.pages.clone(),
        };
        let mut background = vello::Scene::new();
        background.fill(
            vello::peniko::Fill::NonZero,
            vello::kurbo::Affine::IDENTITY,
            vello::peniko::color::palette::css::BLACK,
            None,
            &page().to_kurbo(),
        );
        background.append(
            &build_scene_with(&blank, ViewTransform::default(), options),
            None,
        );
        let pixels = renderer.render(&background).expect("paper");
        assert_eq!(pixel(&pixels, 42, 20), [0; 3]);
        assert_eq!(
            pixel(
                &pixels,
                if mode == 2 {
                    5
                } else if mode == 1 {
                    8
                } else {
                    20
                },
                20
            ),
            [255; 3]
        );
    }
}

#[test]
#[ignore = "needs a GPU adapter; run with -- --ignored"]
fn slug_boundary_is_blue_in_normal_and_absent_in_printing_modes() {
    use tessera_render::scene::{SceneOptions, build_scene_with};
    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let mut doc = empty_doc();
    doc.pages[0].bounds = DocRect {
        x: 20.0,
        y: 20.0,
        width: 40.0,
        height: 40.0,
    };
    doc.pages[0].margins = doc.pages[0].bounds;
    doc.pages[0].bleed = doc.pages[0].bounds;
    doc.pages[0].slug = DocRect {
        x: 10.0,
        y: 10.0,
        width: 60.0,
        height: 60.0,
    };
    let pixels = renderer
        .render(&build_scene(&doc, ViewTransform::default()))
        .expect("normal");
    let guide = pixel(&pixels, 10, 40);
    assert!(
        guide[2] > guide[0] + 20,
        "slug boundary was not blue: {guide:?}"
    );
    for clip in [doc.pages[0].bounds, doc.pages[0].bleed, doc.pages[0].slug] {
        let scene = build_scene_with(
            &doc,
            ViewTransform::default(),
            SceneOptions {
                rules: false,
                clip: Some(vec![clip]),
            },
        );
        let pixels = renderer.render(&scene).expect("printing mode");
        assert_eq!(pixel(&pixels, 10, 40), [255; 3]);
    }
}

#[test]
#[ignore = "needs a GPU adapter; run with -- --ignored"]
fn an_empty_page_renders_white() {
    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let scene = build_scene(&empty_doc(), ViewTransform::default());
    let pixels = renderer.render(&scene).expect("render");

    assert_eq!(pixels.len(), (W * H * 4) as usize, "tightly packed RGBA8");
    assert_eq!(pixel(&pixels, 50, 50), [255, 255, 255], "the page is white");
}

#[test]
#[ignore = "needs a GPU adapter; run with -- --ignored"]
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
#[ignore = "needs a GPU adapter; run with -- --ignored"]
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
#[ignore = "needs a GPU adapter; run with -- --ignored"]
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
        .render(&build_scene(&doc, ViewTransform::default()))
        .expect("render");
    assert_eq!(pixel(&at_origin, 10, 10), [0, 0, 0]);

    // Pan the camera 50pt left, so the rectangle moves right on screen.
    let panned = ViewTransform {
        pan: DocPoint { x: -50.0, y: 0.0 },
        zoom: 1.0,
    };
    let after = renderer.render(&build_scene(&doc, panned)).expect("render");
    assert_eq!(pixel(&after, 10, 10), [255, 255, 255], "moved away");
    assert_eq!(pixel(&after, 60, 10), [0, 0, 0], "moved here");
}

#[test]
#[ignore = "needs a GPU adapter; run with -- --ignored"]
fn text_puts_dark_pixels_on_the_page() {
    let mut shaper = tessera_text::shape::Shaper::new();
    let mut story = tessera_text::story::Story::new("HHHH");
    // Formatting lives on the runs now, and a story starts with exactly one
    // covering its whole text.
    story.runs[0].local.size = Some(48.0);
    let shaped = shaper.shape(&story, &NoStyles::default(), 400.0);
    assert!(shaped.glyph_count() > 0, "the fixture must actually shape");

    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");
    let scene = build_scene(
        &ResolvedDocument {
            pages: vec![resolved_page()],
            items: vec![ResolvedItem {
                frame: FrameId::default(),
                bounds: DocRect {
                    x: 2.0,
                    y: 2.0,
                    width: 400.0,
                    height: 60.0,
                },
                transform: Transform::IDENTITY,
                spread_area: None,
                blend: tessera_document::blending::Blending::PLAIN,
                shadow: None,
                kind: ResolvedKind::Text {
                    shaped,
                    color: Color::BLACK,
                },
            }],
        },
        ViewTransform::default(),
    );
    let pixels = renderer.render(&scene).expect("render");

    let (rgba, _) = pixels.as_chunks::<4>();
    let dark = rgba
        .iter()
        .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
        .count();
    assert!(dark > 20, "glyphs must actually mark the page, saw {dark}");
}

#[test]
#[ignore = "needs a GPU adapter; run with -- --ignored"]
fn rotating_a_bar_moves_the_pixels_it_covers() {
    let mut renderer = HeadlessRenderer::new(W, H).expect("adapter");

    // A wide, short bar across the middle: upright it covers the horizontal
    // centre line and misses the vertical one.
    let bar = DocRect {
        x: 10.0,
        y: 45.0,
        width: 80.0,
        height: 10.0,
    };
    let upright = build_scene(&rect_doc(bar, Color::BLACK), ViewTransform::default());
    let pixels = renderer.render(&upright).expect("render");
    assert_eq!(pixel(&pixels, 20, 50), [0, 0, 0], "upright: across");
    assert_eq!(pixel(&pixels, 50, 20), [255, 255, 255], "upright: not up");

    // Turned a quarter turn about its own centre, those swap.
    let mut doc = rect_doc(bar, Color::BLACK);
    doc.items[0].transform = Transform::rotate_about(90.0, doc.items[0].bounds.center());
    let turned = build_scene(&doc, ViewTransform::default());
    let pixels = renderer.render(&turned).expect("render");
    assert_eq!(pixel(&pixels, 50, 20), [0, 0, 0], "turned: up the page");
    assert_eq!(
        pixel(&pixels, 20, 50),
        [255, 255, 255],
        "turned: not across"
    );
}
