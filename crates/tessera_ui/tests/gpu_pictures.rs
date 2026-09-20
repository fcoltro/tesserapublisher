//! A placed picture, through the application: placed by command, resolved
//! from the document, drawn by the renderer. GPU-backed and `#[ignore]`d for
//! the reasons `tessera_render/tests/gpu_render.rs` gives; run alone:
//!
//! ```text
//! cargo test -p tessera_ui --test gpu_pictures -- --ignored
//! ```
//!
//! Through the application rather than a hand-built resolved document,
//! because that is where the fault was: the renderer's own test placed its
//! frame beside the origin and passed while every frame on a real page,
//! hundreds of points from the origin, drew nothing.

use tessera_ui::app::TesseraApp;
use tessera_ui::command::{Command, apply};

#[test]
#[ignore = "needs a GPU adapter; run with -- --ignored"]
fn a_picture_placed_by_command_is_painted_in_its_frame() {
    let art = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/tessera-publisher-logotype.png");
    let mut app = TesseraApp::headless();
    let page = app.first_page_bounds();
    let frame = tessera_geometry::DocRect {
        x: page.x + 50.0,
        y: page.y + 50.0,
        width: 200.0,
        height: 200.0,
    };
    apply(&mut app, Command::AddGraphicFrame(frame));
    let id = app.active().selection.single().expect("selected");
    apply(
        &mut app,
        Command::PlaceArtwork {
            id,
            path: art,
            fit: tessera_document::graphic::Fit::Proportionally,
        },
    );

    // The page at half size, the way the bridge's render_page draws it.
    let zoom = 0.5;
    let width = (page.width * zoom) as u32;
    let height = (page.height * zoom) as u32;
    let view = tessera_geometry::ViewTransform {
        pan: tessera_geometry::DocPoint {
            x: page.x,
            y: page.y,
        },
        zoom,
    };
    let resolved = app.resolve_uncached();
    let scene = tessera_render::scene::build_scene_with_images(
        &resolved,
        view,
        tessera_render::scene::SceneOptions {
            rules: false,
            clip: Some(vec![page]),
        },
        &mut app.images,
    );
    let mut renderer = tessera_render::HeadlessRenderer::new(width, height).expect("renderer");
    let pixels = renderer.render(&scene).expect("render");
    let painted = |x0: u32, y0: u32, x1: u32, y1: u32| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| ((y * width + x) * 4) as usize))
            .filter(|&i| pixels[i] != 255 || pixels[i + 1] != 255 || pixels[i + 2] != 255)
            .count()
    };
    // The frame, on the rendered page: 25..125 both ways.
    assert!(
        painted(25, 25, 125, 125) > 200,
        "the picture is not in its frame"
    );
    assert_eq!(
        painted(0, 0, 20, 20),
        0,
        "ink at the page's corner, where nothing is"
    );
}
