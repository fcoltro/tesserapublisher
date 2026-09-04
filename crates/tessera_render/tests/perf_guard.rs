//! An order-of-magnitude guard on the interactive path.
//!
//! The budget in the spec is 16.7 ms per whole frame. The ceiling here is that
//! budget rounded up, which gives the number a meaning: **this stage alone must
//! never consume an entire frame.** Everything else a frame does — input,
//! egui's own layout, the GPU submission — has to fit alongside it.
//!
//! It is deliberately not tighter. The measured time on the machine this was
//! written on is around 0.4 ms, so there is roughly fifty times of headroom;
//! a shared CI runner under load can be many times slower than a desktop
//! without anything being wrong, and a flaky performance test gets muted,
//! which is worse than no test at all.
//!
//! No GPU: `build_scene` is CPU work, so this runs in the ordinary suite.

use std::time::Instant;

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::nodes::{Frame, FrameKind};
use tessera_geometry::{DocRect, Transform, ViewTransform};
use tessera_text::shape::Shaper;

const FRAMES: usize = 500;
const CEILING_MILLIS: u128 = 20;

fn crowded_document() -> Document {
    let mut document = Document::new();
    let layer = document
        .default_layer()
        .expect("a new document has a layer");

    for i in 0..FRAMES {
        let across = (i % 25) as f64;
        let down = (i / 25) as f64;
        document.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: 10.0 + across * 22.0,
                    y: 10.0 + down * 28.0,
                    width: 18.0,
                    height: 24.0,
                },
                transform: Transform::IDENTITY,
                kind: FrameKind::Rectangle,
                fill: Color::BLACK,
                stroke: None,
            },
        );
    }

    document
}

#[test]
fn resolving_and_building_five_hundred_frames_stays_fast() {
    let document = crowded_document();
    let page = document.first_page_bounds();
    let view = ViewTransform::default();
    let mut shaper = Shaper::new();

    // One untimed pass, so that lazily built caches are not charged to the
    // measurement.
    let warm = tessera_layout::resolve(&document, &mut shaper);
    let _ = tessera_render::scene::build_scene(&warm, view, page);

    let started = Instant::now();
    let resolved = tessera_layout::resolve(&document, &mut shaper);
    let _scene = tessera_render::scene::build_scene(&resolved, view, page);
    let elapsed = started.elapsed();

    println!(
        "resolve + build_scene over {FRAMES} frames: {:.2} ms",
        elapsed.as_secs_f64() * 1000.0
    );

    assert!(
        elapsed.as_millis() < CEILING_MILLIS,
        "took {} ms for {FRAMES} frames, ceiling is {CEILING_MILLIS} ms — \
         something on the interactive path has become far slower",
        elapsed.as_millis()
    );
}
