//! The `.tessera` format's round-trip guarantee.
//!
//! Non-negotiable N1: a document can be saved, closed, and reopened
//! faithfully. The property test at the bottom is what makes that structural
//! rather than hopeful — it generates arbitrary documents and asserts that
//! save-then-load is the identity.

use proptest::prelude::*;
use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::format;
use tessera_document::nodes::{Frame, FrameKind, Stroke};
use tessera_geometry::DocRect;

fn temp_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("tessera_format_tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

#[test]
fn an_empty_document_round_trips() {
    let path = temp_path("empty.tessera");
    let doc = Document::new();

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    assert_eq!(loaded.spread_order.len(), doc.spread_order.len());
    assert_eq!(loaded.frames.len(), doc.frames.len());
    assert_eq!(loaded.layer_ids().count(), doc.layer_ids().count());
}

#[test]
fn a_document_with_a_rectangle_round_trips_exactly() {
    let path = temp_path("rect.tessera");
    let mut doc = Document::new();
    let layer = doc.default_layer().expect("default layer");
    let id = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 1.5,
                y: 2.5,
                width: 300.0,
                height: 200.0,
            },
            kind: FrameKind::Rectangle,
            fill: Color::Cmyk {
                c: 0.1,
                m: 0.2,
                y: 0.3,
                k: 0.4,
                a: 1.0,
            },
            stroke: Some(Stroke {
                color: Color::BLACK,
                width: 2.0,
            }),
        },
    );

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    assert_eq!(
        loaded.frame(id).expect("frame survived"),
        doc.frame(id).expect("original")
    );
}

#[test]
fn the_archive_carries_a_meta_entry() {
    let path = temp_path("meta.tessera");
    format::save(&Document::new(), &path).expect("save");

    let file = std::fs::File::open(&path).expect("open");
    let mut zip = zip::ZipArchive::new(file).expect("valid zip");

    assert!(
        zip.by_name("meta.json").is_ok(),
        "meta.json must be present"
    );
    assert!(
        zip.by_name("document.json").is_ok(),
        "document.json must be present"
    );
}

#[test]
fn a_newer_format_version_is_refused_rather_than_guessed_at() {
    let path = temp_path("future.tessera");
    format::save(&Document::new(), &path).expect("save");
    format::rewrite_version_for_test(&path, format::FORMAT_VERSION + 1).expect("rewrite");

    match format::load(&path) {
        Err(format::FormatError::NewerFormat { found, supported }) => {
            assert_eq!(found, format::FORMAT_VERSION + 1);
            assert_eq!(supported, format::FORMAT_VERSION);
        }
        other => panic!("expected NewerFormat, got {other:?}"),
    }
}

#[test]
fn a_file_that_is_not_an_archive_is_reported_not_panicked() {
    let path = temp_path("garbage.tessera");
    std::fs::write(&path, b"this is not a zip file").expect("write");

    assert!(matches!(
        format::load(&path),
        Err(format::FormatError::Archive(_))
    ));
}

#[test]
fn a_missing_file_is_reported() {
    let path = temp_path("definitely_absent.tessera");
    let _ = std::fs::remove_file(&path);

    assert!(matches!(
        format::load(&path),
        Err(format::FormatError::Read(_))
    ));
}

// --- the property test -------------------------------------------------

fn any_color() -> impl Strategy<Value = Color> {
    prop_oneof![
        (0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0).prop_map(|(r, g, b)| Color::Rgb {
            r,
            g,
            b,
            a: 1.0
        }),
        (0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0, 0.0f32..1.0).prop_map(|(c, m, y, k)| Color::Cmyk {
            c,
            m,
            y,
            k,
            a: 1.0
        }),
    ]
}

fn any_frame() -> impl Strategy<Value = Frame> {
    (
        -1000.0f64..1000.0,
        -1000.0f64..1000.0,
        1.0f64..1000.0,
        1.0f64..1000.0,
        any_color(),
        prop::option::of(0.1f64..20.0),
    )
        .prop_map(|(x, y, width, height, fill, stroke_width)| Frame {
            bounds: DocRect {
                x,
                y,
                width,
                height,
            },
            kind: FrameKind::Rectangle,
            fill,
            stroke: stroke_width.map(|width| Stroke {
                color: Color::BLACK,
                width,
            }),
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn any_document_survives_a_save_and_load(
        frames in prop::collection::vec(any_frame(), 0..12)
    ) {
        let path = temp_path("proptest.tessera");
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let ids: Vec<_> = frames.into_iter().map(|f| doc.add_frame(layer, f)).collect();

        format::save(&doc, &path).expect("save");
        let loaded = format::load(&path).expect("load");

        prop_assert_eq!(loaded.frames.len(), doc.frames.len());
        for id in ids {
            prop_assert_eq!(loaded.frame(id), doc.frame(id));
        }
        prop_assert_eq!(loaded.paint_order(), doc.paint_order());
    }
}

#[test]
fn text_survives_a_save_and_load() {
    // The bug this pins: stories once lived beside the document rather than
    // inside it, so a saved file kept the text FRAMES and silently dropped the
    // text. Everything looked right until the file was reopened.
    let path = temp_path("text.tessera");
    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let story = doc.add_story(tessera_text::story::Story::new("Hello, Tessera."));
    let frame = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 10.0,
                y: 10.0,
                width: 400.0,
                height: 40.0,
            },
            kind: FrameKind::Text { story },
            fill: Color::WHITE,
            stroke: None,
        },
    );

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    let FrameKind::Text {
        story: loaded_story,
    } = loaded.frame(frame).expect("frame survived").kind.clone()
    else {
        panic!("the frame must still be a text frame");
    };
    assert_eq!(
        loaded.story(loaded_story).expect("story survived").text,
        "Hello, Tessera.",
        "the text itself must come back, not just the frame holding it"
    );
}
