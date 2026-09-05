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
    let view = ViewTransform::default();
    let mut shaper = Shaper::new();

    // One untimed pass, so that lazily built caches are not charged to the
    // measurement.
    let warm = tessera_layout::resolve(&document, &mut shaper);
    let _ = tessera_render::scene::build_scene(&warm, view);

    let started = Instant::now();
    let resolved = tessera_layout::resolve(&document, &mut shaper);
    let _scene = tessera_render::scene::build_scene(&resolved, view);
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

// --- text ---------------------------------------------------------------

const PARAGRAPHS: usize = 60;
/// Text is the expensive path, so it gets more headroom than the shapes do —
/// but not so much that a real regression hides in it.
const TEXT_CEILING_MILLIS: u128 = 60;

/// A document of text frames, each story broken into alternating runs.
///
/// Runs are the point: milestone 2 made shaping resolve a format per span and
/// push a parley style span for each, and the rectangle guard above would not
/// notice any of that getting slower.
fn wordy_document() -> Document {
    use tessera_document::nodes::FrameKind;
    use tessera_text::story::{CharacterFormat, Run, Story};

    let mut document = Document::new();
    let layer = document
        .default_layer()
        .expect("a new document has a layer");

    for i in 0..PARAGRAPHS {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(4);
        let mut story = Story::new(text);

        // Alternating sizes, so every run really differs from its neighbour
        // and none of them can be merged away.
        let len = story.text.len();
        let mut runs = Vec::new();
        let mut at = 0;
        let mut big = true;
        while at < len {
            let end = (at + 11).min(len);
            runs.push(Run {
                range: at..end,
                style: None,
                local: CharacterFormat {
                    size: Some(if big { 14.0 } else { 10.0 }),
                    ..CharacterFormat::default()
                },
            });
            at = end;
            big = !big;
        }
        story.runs = runs;

        let id = document.add_story(story);
        document.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: 20.0,
                    y: 20.0 + (i as f64) * 12.0,
                    width: 400.0,
                    height: 60.0,
                },
                transform: Transform::IDENTITY,
                kind: FrameKind::Text { story: id },
                fill: Color::BLACK,
                stroke: None,
            },
        );
    }

    document
}

#[test]
fn shaping_a_wordy_document_stays_fast() {
    let document = wordy_document();
    let view = ViewTransform::default();
    let mut shaper = Shaper::new();

    // Untimed, so the font collection and the shaping cache are not charged
    // to the measurement.
    let warm = tessera_layout::resolve(&document, &mut shaper);
    let _ = tessera_render::scene::build_scene(&warm, view);

    let started = Instant::now();
    let resolved = tessera_layout::resolve(&document, &mut shaper);
    let _scene = tessera_render::scene::build_scene(&resolved, view);
    let elapsed = started.elapsed();

    println!(
        "resolve + build_scene over {PARAGRAPHS} run-broken paragraphs: {:.2} ms",
        elapsed.as_secs_f64() * 1000.0
    );

    assert!(
        elapsed.as_millis() < TEXT_CEILING_MILLIS,
        "took {} ms for {PARAGRAPHS} paragraphs, ceiling is {TEXT_CEILING_MILLIS} ms —          shaping runs has become far more expensive",
        elapsed.as_millis()
    );
}
