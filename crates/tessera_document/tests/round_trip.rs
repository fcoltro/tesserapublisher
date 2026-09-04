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
use tessera_document::nodes::{
    Axis, DocumentSetup, Frame, FrameKind, Guide, Insets, Margins, Stroke,
};
use tessera_geometry::{DocPoint, DocRect, Transform};

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
            transform: Transform::IDENTITY,
            fill: Color::Cmyk {
                c: 0.1,
                m: 0.2,
                y: 0.3,
                k: 0.4,
                a: 1.0,
            },
            stroke: Some(Stroke::new(Color::BLACK, 2.0)),
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
            transform: Transform::IDENTITY,
            fill,
            stroke: stroke_width.map(|width| Stroke::new(Color::BLACK, width)),
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
            transform: Transform::IDENTITY,
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

#[test]
fn a_version_1_document_still_opens() {
    // A hand-built archive in the format as it stood before frames had a
    // rotation. The migration chain is only real if something actually
    // travels it, so this fixture is written by hand rather than by the
    // current writer.
    use std::io::Write;

    let path = temp_path("legacy_v1.tessera");

    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 5.0,
                y: 6.0,
                width: 70.0,
                height: 80.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: Color::BLACK,
            stroke: None,
        },
    );

    // Serialise, then strip every `rotation` key back out to make it a
    // version 1 file. Walking the tree rather than assuming a shape: the
    // arenas are slotmaps, whose serialised form is not an object of frames.
    fn strip_transform(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("transform");
                for v in map.values_mut() {
                    strip_transform(v);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    strip_transform(v);
                }
            }
            _ => {}
        }
    }

    let mut value: serde_json::Value = serde_json::to_value(&doc).expect("to value");
    strip_transform(&mut value);
    let body = serde_json::to_vec(&value).expect("body");
    assert!(
        !String::from_utf8_lossy(&body).contains("transform"),
        "the fixture must genuinely lack the field"
    );

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("meta.json", options).expect("meta");
        zip.write_all(br#"{"format_version":1,"app_version":"0.1.0","created":"","modified":""}"#)
            .expect("meta body");
        zip.start_file("document.json", options).expect("doc");
        zip.write_all(&body).expect("doc body");
        zip.finish().expect("finish");
    }
    std::fs::write(&path, buffer.into_inner()).expect("write fixture");

    let loaded = format::load(&path).expect("a version 1 document must still open");

    let frame = loaded.frame(id).expect("frame survived");
    assert_eq!(frame.bounds.width, 70.0);
    assert!(
        frame.transform.is_identity(),
        "a document written before placements existed loads unplaced"
    );
}

#[test]
fn a_placement_survives_a_save_and_load() {
    let path = temp_path("rotated.tessera");
    // Sheared as well as turned, so this cannot pass by carrying an angle:
    // all six coefficients have to survive the round trip.
    let placed = Transform::rotate_about(33.5, DocPoint { x: 5.0, y: 5.0 })
        .then(Transform::scale_about(2.0, 1.0, DocPoint::ZERO));
    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            kind: FrameKind::Rectangle,
            transform: placed,
            fill: Color::BLACK,
            stroke: None,
        },
    );

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    assert_eq!(loaded.frame(id).expect("frame").transform, placed);
}

#[test]
fn a_version_2_rotation_becomes_the_placement_that_means_the_same_thing() {
    // The first migration that rewrites rather than relying on serde
    // defaults. A frame's rotation was always about its own centre, so the
    // document must come back with its corners in the same places -- which is
    // what this checks, rather than checking the representation.
    use std::io::Write as _;

    let path = temp_path("v2-rotation.tessera");
    let bounds = DocRect {
        x: 40.0,
        y: 10.0,
        width: 100.0,
        height: 20.0,
    };
    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            bounds,
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: Color::BLACK,
            stroke: None,
        },
    );

    const DEGREES: f64 = 33.5;

    /// Put the old field back, exactly as version 2 wrote it.
    fn downgrade(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if map.remove("transform").is_some() {
                    map.insert("rotation".to_string(), DEGREES.into());
                }
                for v in map.values_mut() {
                    downgrade(v);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    downgrade(v);
                }
            }
            _ => {}
        }
    }

    let mut value: serde_json::Value = serde_json::to_value(&doc).expect("to value");
    downgrade(&mut value);
    let body = serde_json::to_vec(&value).expect("body");
    assert!(
        String::from_utf8_lossy(&body).contains("rotation"),
        "the fixture must genuinely carry the old field"
    );

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("meta.json", options).expect("meta");
        zip.write_all(br#"{"format_version":2,"app_version":"0.1.0","created":"","modified":""}"#)
            .expect("meta body");
        zip.start_file("document.json", options).expect("doc");
        zip.write_all(&body).expect("doc body");
        zip.finish().expect("finish");
    }
    std::fs::write(&path, buffer.into_inner()).expect("write fixture");

    let loaded = format::load(&path).expect("a version 2 document must still open");
    let frame = loaded.frame(id).expect("frame survived");

    assert_eq!(frame.bounds, bounds, "the box itself does not move");
    for (corner, was) in frame.corners().into_iter().zip([
        DocPoint { x: 40.0, y: 10.0 },
        DocPoint { x: 140.0, y: 10.0 },
        DocPoint { x: 140.0, y: 30.0 },
        DocPoint { x: 40.0, y: 30.0 },
    ]) {
        // Where the old model would have put it: rotated about its own centre.
        let expected = was.rotated_about(bounds.center(), DEGREES);
        assert!(
            (corner.x - expected.x).abs() < 1e-9 && (corner.y - expected.y).abs() < 1e-9,
            "corner landed at {corner:?}, the old model says {expected:?}"
        );
    }
}


// --- page setup, guides, and the version-5 bump -------------------------

#[test]
fn page_setup_and_guides_survive_a_round_trip() {
    // They are document data now, so the round-trip guarantee covers them.
    let path = temp_path("page-setup-round-trip.tessera");
    let _ = std::fs::remove_file(&path);

    let mut original = Document::new();
    original.setup = DocumentSetup {
        margins: Margins {
            top: 36.0,
            bottom: 42.0,
            inside: 60.0,
            outside: 24.0,
        },
        bleed: Insets::uniform(9.0),
        slug: Insets {
            top: 18.0,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        },
        facing_pages: true,
    };
    let spread = original.spread_ids().next().expect("a spread");
    original.add_guide(
        spread,
        Guide {
            axis: Axis::Vertical,
            position: 123.5,
            locked: true,
        },
    );

    format::save(&original, &path).expect("save");
    let reopened = format::load(&path).expect("load");

    assert_eq!(reopened.setup, original.setup, "the setup came back");
    assert_eq!(
        reopened.guides_of(spread),
        original.guides_of(spread),
        "and so did the guide"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_version_four_document_still_opens_and_gains_no_setup_it_never_had() {
    // Everything phase B added carries serde(default), so a version-4
    // document needs no rewriting. "Needs no rewriting" is a claim; this is
    // the test that lets it fail.
    let path = temp_path("v4-migration.tessera");
    let _ = std::fs::remove_file(&path);

    let mut original = Document::new();
    let layer = original.default_layer().expect("a layer");
    original.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 12.0,
                y: 34.0,
                width: 56.0,
                height: 78.0,
            },
            transform: Transform::IDENTITY,
            kind: FrameKind::Rectangle,
            fill: Color::BLACK,
            stroke: None,
        },
    );

    format::save(&original, &path).expect("save");
    format::rewrite_version_for_test(&path, 4).expect("stamp it as version 4");

    let reopened = format::load(&path).expect("a version-4 document still opens");
    assert_eq!(reopened.frames.len(), 1, "its frames survived");
    assert_eq!(
        reopened.setup,
        DocumentSetup::default(),
        "and it gained no margins it never had"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_document_from_a_newer_build_is_refused_rather_than_guessed_at() {
    let path = temp_path("v99-refusal.tessera");
    let _ = std::fs::remove_file(&path);

    format::save(&Document::new(), &path).expect("save");
    format::rewrite_version_for_test(&path, 99).expect("stamp");

    let error = format::load(&path).expect_err("a newer format must be refused");
    assert!(
        matches!(error, format::FormatError::NewerFormat { found: 99, .. }),
        "got {error}"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_format_version_is_five() {
    // Phase B's one bump. If this changes, a migration step is owed.
    assert_eq!(format::FORMAT_VERSION, 5);
}
