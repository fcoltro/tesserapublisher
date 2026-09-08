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
use tessera_document::paint::Paint;
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
            fill: Paint::Solid(Color::Cmyk {
                c: 0.1,
                m: 0.2,
                y: 0.3,
                k: 0.4,
                a: 1.0,
            }),
            stroke: Some(Stroke::new(Color::BLACK, 2.0)),
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
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
            fill: Paint::Solid(fill),
            stroke: stroke_width.map(|width| Stroke::new(Color::BLACK, width)),
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
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
            kind: FrameKind::text(story),
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::WHITE),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
        },
    );

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    let FrameKind::Text {
        story: loaded_story,
        ..
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
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
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
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
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
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
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
        baseline_grid: None,
        columns: 3,
        column_gutter: 14.0,
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

/// Build a version-4 archive by hand: a document with **no `setup` block at
/// all**, which is what one written before phase B looked like.
///
/// `rewrite_version_for_test` cannot be used. It loads with the current model
/// and re-saves under an older stamp, so the JSON it produces still carries
/// every field the current model has — the version number says 4 and nothing
/// else does. The test below passed for a year that way, and only failed when
/// `Document::new` began choosing something `DocumentSetup::default()` does
/// not.
fn version_4_archive(doc: &Document, path: &std::path::Path) {
    use std::io::Write;

    let mut value: serde_json::Value = serde_json::to_value(doc).expect("to value");
    if let Some(map) = value.as_object_mut() {
        map.remove("setup");
    }
    let body = serde_json::to_vec(&value).expect("body");
    assert!(
        !String::from_utf8_lossy(&body).contains("\"setup\""),
        "the fixture must genuinely lack the field it is testing the absence of"
    );

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("meta.json", options).expect("meta");
        zip.write_all(br#"{"format_version":4,"app_version":"0.1.0","created":"","modified":""}"#)
            .expect("meta body");
        zip.start_file("document.json", options).expect("doc");
        zip.write_all(&body).expect("doc body");
        zip.finish().expect("finish");
    }
    std::fs::write(path, buffer.into_inner()).expect("write fixture");
}

#[test]
fn a_version_four_document_still_opens_and_gains_no_setup_it_never_had() {
    // Everything phase B added carries serde(default), so a version-4 document
    // needs no rewriting. "Needs no rewriting" is a claim; this is the test
    // that lets it fail — and it can only fail if the fixture really has no
    // setup, which is why it is built by hand.
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
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
        },
    );

    version_4_archive(&original, &path);

    let reopened = format::load(&path).expect("a version-4 document still opens");
    assert_eq!(reopened.frames.len(), 1, "its frames survived");
    assert_eq!(
        reopened.setup,
        DocumentSetup::default(),
        "and it gained no setup it never had"
    );
    assert!(
        !reopened.setup.facing_pages,
        "least of all facing pages, which would rearrange its spreads on open"
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
fn the_format_version_is_nineteen() {
    // A tripwire, not a fact worth asserting on its own: changing it means
    // stopping to ask whether a migration step is owed. Sometimes the answer is
    // no — version 19 added `corners`, whose default is exactly what older
    // documents meant — and the point is that somebody had to answer.
    assert_eq!(format::FORMAT_VERSION, 19);
}

#[test]
fn a_frame_written_before_corners_reads_as_square() {
    // The reason version 19 needs no migration step: the default *is* what
    // those documents meant. Asserted against the serialised form with the
    // field taken back out, which is exactly the shape version 18 wrote.
    let frame = tessera_document::nodes::Frame {
        bounds: DocRect {
            x: 10.0,
            y: 10.0,
            width: 60.0,
            height: 40.0,
        },
        transform: Transform::IDENTITY,
        kind: tessera_document::nodes::FrameKind::Rectangle,
        fill: Paint::Solid(Color::BLACK),
        stroke: None,
        wrap: tessera_document::nodes::TextWrap::None,
        blend: tessera_document::blending::Blending::PLAIN,
        corners: tessera_document::corners::Corners::SQUARE,
        shadow: None,
        style: None,
    };

    let mut written: serde_json::Value = serde_json::to_value(&frame).expect("write");
    written
        .as_object_mut()
        .expect("a frame is an object")
        .remove("corners")
        .expect("corners were not written at all");

    let back: tessera_document::nodes::Frame =
        serde_json::from_value(written).expect("a frame without corners would not read");
    assert!(back.corners.is_square(), "corners arrived from nowhere");
}

#[test]
fn an_output_intent_travels_in_the_document_with_its_profile() {
    // **The profile itself, not a path to one.** A document recording
    // "C:/profiles/FOGRA39.icc" would mean something different on the printer’s
    // machine than on the designer’s, and that is exactly where being wrong is
    // expensive.
    use tessera_document::intent::{OutputIntent, Rendering};

    let path = temp_path("output_intent.tessera");
    let _ = std::fs::remove_file(&path);

    let profile = tessera_color::managed::OutputProfile::screen().expect("a profile");
    let intent = OutputIntent {
        description: profile.description().to_string(),
        profile: profile.bytes().to_vec(),
        rendering: Rendering::Perceptual,
    };

    let mut doc = Document::new();
    doc.output_intent = Some(intent.clone());

    format::save(&doc, &path).expect("save");
    let back = format::load(&path).expect("load");
    let loaded = back.output_intent.expect("the intent");

    assert_eq!(loaded.profile, intent.profile, "byte for byte");
    assert_eq!(loaded.rendering, Rendering::Perceptual);
    assert!(!loaded.description.is_empty());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_document_written_before_output_intents_has_no_press_rather_than_a_guessed_one() {
    // Inventing sRGB would show every old document proofed against a decision
    // its author never made, and the colours would be believed.
    let path = temp_path("no_intent.tessera");
    let _ = std::fs::remove_file(&path);

    format::save(&Document::new(), &path).expect("save");
    let back = format::load(&path).expect("load");
    assert!(back.output_intent.is_none());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn object_styles_and_the_objects_following_them_round_trip() {
    use tessera_document::object_style::{ObjectFormat, ObjectStyle};
    use tessera_document::paint::Paint;

    let path = temp_path("object_styles.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            transform: Default::default(),
            kind: FrameKind::Rectangle,
            fill: Paint::Solid(Color::default()),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
        },
    );

    let base = doc.add_object_style(ObjectStyle::new("Panel"));
    let style = doc.add_object_style(ObjectStyle {
        name: "Caption box".to_string(),
        based_on: Some(base),
        format: ObjectFormat {
            shadow: Some(Some(tessera_document::shadow::Shadow::TYPICAL)),
            stroke: Some(None),
            ..Default::default()
        },
    });
    doc.apply_object_style(id, style);

    format::save(&doc, &path).expect("save");
    let back = format::load(&path).expect("load");

    // The order itself, not a count of it. Counting to two was a proxy that
    // broke when a new document started with a style of its own, and that would
    // have gone on passing if the order had been reversed.
    assert_eq!(
        back.object_style_order, doc.object_style_order,
        "and in order"
    );
    let loaded = back
        .object_styles
        .get(style)
        .expect("the style, under the same key");
    assert_eq!(loaded.name, "Caption box");
    assert_eq!(loaded.based_on, Some(base), "the chain, not a copy");
    assert_eq!(
        loaded.format.stroke,
        Some(None),
        "saying no stroke must survive: it is a value, not a silence"
    );
    assert_eq!(
        back.frame(id).expect("frame").style,
        Some(style),
        "and the object still follows it"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_drop_shadow_round_trips() {
    use tessera_document::shadow::Shadow;

    let path = temp_path("shadowed.tessera");
    let _ = std::fs::remove_file(&path);

    let shadow = Shadow {
        offset: (-3.5, 6.25),
        blur: 8.0,
        colour: Color::Rgb {
            r: 0.1,
            g: 0.1,
            b: 0.2,
            a: 0.4,
        },
    };

    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 30.0,
            },
            transform: Default::default(),
            kind: FrameKind::Rectangle,
            fill: Paint::Solid(Color::default()),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: Some(shadow.clone()),
            style: None,
        },
    );

    format::save(&doc, &path).expect("save");
    let back = format::load(&path).expect("load");
    assert_eq!(back.frame(id).expect("frame").shadow, Some(shadow));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_gradient_fill_round_trips() {
    use tessera_document::paint::{Gradient, Paint, Ramp, Stop};

    let path = temp_path("gradient.tessera");
    let _ = std::fs::remove_file(&path);

    let ramp = Gradient::new(
        Ramp::Linear { angle: 30.0 },
        vec![
            Stop {
                at: 0.0,
                colour: Color::BLACK,
            },
            Stop {
                at: 0.4,
                colour: Color::WHITE,
            },
            Stop {
                at: 1.0,
                colour: Color::BLACK,
            },
        ],
    );

    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            },
            transform: Default::default(),
            kind: FrameKind::Rectangle,
            fill: Paint::Gradient(ramp.clone()),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
        },
    );

    format::save(&doc, &path).expect("save");
    let back = format::load(&path).expect("load");
    assert_eq!(
        back.frame(id).expect("frame").fill,
        Paint::Gradient(ramp),
        "the whole ramp, its angle and every stop"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_document_written_before_gradients_opens_with_its_colour_intact() {
    // **The migration that has to rewrite.** A colour and a paint are different
    // shapes on disk, so without this step serde refuses the document and every
    // file written before gradients stops opening.
    use tessera_document::paint::Paint;

    let teal = Color::Cmyk {
        c: 0.8,
        m: 0.2,
        y: 0.3,
        k: 0.0,
        a: 1.0,
    };
    let path = temp_path("legacy_v14.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 3.0,
                y: 4.0,
                width: 20.0,
                height: 10.0,
            },
            transform: Default::default(),
            kind: FrameKind::Rectangle,
            fill: Paint::Solid(teal.clone()),
            stroke: Some(tessera_document::nodes::Stroke::new(Color::BLACK, 2.0)),
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            style: None,
        },
    );
    format::save(&doc, &path).expect("save");

    // Put the fill back into the shape version 14 wrote, and say it was 14.
    format::unwrap_fills_and_stamp_for_test(&path, 14).expect("downgrade");

    let back = format::load(&path).expect("an older document must still open");
    let frame = back.frame(id).expect("frame");
    assert_eq!(frame.fill, Paint::Solid(teal), "the colour, not a default");
    assert_eq!(
        frame.stroke.as_ref().expect("stroke").color,
        Color::BLACK,
        "and a stroke colour was not wrapped: a stroke is still a colour"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_objects_opacity_and_blend_mode_round_trip() {
    use tessera_document::blending::{BlendMode, Blending};

    let path = temp_path("blended.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            corners: tessera_document::corners::Corners::SQUARE,
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            },
            transform: Default::default(),
            kind: FrameKind::Rectangle,
            fill: Paint::Solid(Color::default()),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: Blending {
                opacity: 0.375,
                mode: BlendMode::Multiply,
            },
            shadow: None,
            style: None,
        },
    );

    format::save(&doc, &path).expect("save");
    let back = format::load(&path).expect("load");
    let frame = back.frame(id).expect("the frame");
    assert_eq!(frame.blend.mode, BlendMode::Multiply);
    assert!(
        (frame.blend.opacity - 0.375).abs() < 1e-6,
        "got {}",
        frame.blend.opacity
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_document_written_before_opacity_existed_opens_fully_opaque() {
    // The default is the truth about an older document, not a fabrication:
    // every object in it was painted over, solid.
    use tessera_document::blending::Blending;

    let json = serde_json::json!({
        "bounds": { "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0 },
        "kind": "Rectangle",
        "fill": { "Solid": { "Rgb": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 } } },
        "stroke": null
    });
    let frame: Frame = serde_json::from_value(json).expect("an older frame");
    assert_eq!(frame.blend, Blending::PLAIN);
}

#[test]
fn placed_artwork_round_trips_as_a_link_rather_than_as_pixels() {
    // Linked, never embedded: what is saved is a path and what the document
    // last knew about it.
    use tessera_document::graphic::Fit;
    use tessera_document::links::Link;

    let path = temp_path("placed.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            corners: tessera_document::corners::Corners::SQUARE,
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            },
            kind: FrameKind::Graphic { placed: None },
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            style: None,
        },
    );
    let link = doc.add_link(Link::new("C:/art/photo.png", (640.0, 480.0)));
    assert!(doc.place(id, link, Fit::Proportionally));

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    assert_eq!(loaded.links.len(), 1);
    let (_, saved) = loaded.links.iter().next().expect("a link");
    assert_eq!(saved.path, std::path::PathBuf::from("C:/art/photo.png"));
    assert_eq!(saved.natural, (640.0, 480.0));

    let FrameKind::Graphic { placed: Some(p) } = loaded.frame(id).expect("frame").kind.clone()
    else {
        panic!("the artwork did not come back");
    };
    assert!(!p.inner.is_identity(), "and it is still fitted");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_version_twelve_document_opens_with_no_links() {
    let path = temp_path("legacy_v12.tessera");
    let _ = std::fs::remove_file(&path);

    let doc = Document::new();
    format::save(&doc, &path).expect("save");
    format::rewrite_version_for_test(&path, 12).expect("stamp");

    let loaded = format::load(&path).expect("a version 12 document must still open");
    assert!(loaded.links.is_empty());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn swatches_and_the_objects_naming_them_round_trip() {
    use tessera_document::nodes::Swatch;

    let path = temp_path("swatches.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    doc.set_swatch(Swatch {
        name: "Brand red".to_string(),
        colour: Color::Cmyk {
            c: 0.0,
            m: 0.9,
            y: 0.8,
            k: 0.0,
            a: 1.0,
        },
        spot: true,
    });

    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            corners: tessera_document::corners::Corners::SQUARE,
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::Swatch {
                name: "Brand red".to_string(),
                tint: 0.5,
            }),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            style: None,
        },
    );

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    assert_eq!(loaded.swatches.len(), 1);
    assert!(loaded.swatch("Brand red").expect("a swatch").spot);
    let fill = loaded.frame(id).expect("frame").fill.clone();
    assert_eq!(
        fill,
        Paint::Solid(Color::Swatch {
            name: "Brand red".to_string(),
            tint: 0.5
        }),
        "the object still holds the name, not the value"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_version_eleven_document_opens_with_no_swatches() {
    let path = temp_path("legacy_v11.tessera");
    let _ = std::fs::remove_file(&path);

    let doc = Document::new();
    format::save(&doc, &path).expect("save");
    format::rewrite_version_for_test(&path, 11).expect("stamp");

    let loaded = format::load(&path).expect("a version 11 document must still open");
    assert!(loaded.swatches.is_empty());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_baseline_grid_round_trips() {
    use tessera_document::nodes::BaselineGrid;

    let path = temp_path("grid.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    doc.setup.baseline_grid = Some(BaselineGrid {
        start: 12.0,
        step: 14.4,
    });
    format::save(&doc, &path).expect("save");

    let loaded = format::load(&path).expect("load");
    assert_eq!(
        loaded.setup.baseline_grid,
        Some(BaselineGrid {
            start: 12.0,
            step: 14.4
        })
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_version_ten_document_opens_with_no_grid() {
    // `None` is the truth about a document written before grids existed, and
    // `false` about every frame in it.
    let path = temp_path("legacy_v10.tessera");
    let _ = std::fs::remove_file(&path);

    let doc = Document::new();
    format::save(&doc, &path).expect("save");
    format::rewrite_version_for_test(&path, 10).expect("stamp");

    let loaded = format::load(&path).expect("a version 10 document must still open");
    assert_eq!(loaded.setup.baseline_grid, None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_version_nine_text_frame_opens_as_a_single_column() {
    // 9 -> 10 rewrites nothing: `TextLayout::default()` is one column with no
    // inset aligned to the top, which is exactly what a text frame written
    // before columns existed did.
    use tessera_document::nodes::{TextLayout, VerticalJustify};

    let path = temp_path("legacy_v9.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let story = doc.add_story(tessera_text::story::Story::new("some copy"));
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            corners: tessera_document::corners::Corners::SQUARE,
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 60.0,
            },
            kind: FrameKind::text(story),
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            style: None,
        },
    );

    format::save(&doc, &path).expect("save");
    format::rewrite_version_for_test(&path, 9).expect("stamp");

    let loaded = format::load(&path).expect("a version 9 document must still open");
    let FrameKind::Text { layout, .. } = loaded.frame(id).expect("frame").kind.clone() else {
        panic!("a text frame came back as something else");
    };

    assert_eq!(layout, TextLayout::default());
    assert_eq!(layout.columns, 1);
    assert_eq!(layout.vertical, VerticalJustify::Top);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_columned_text_frame_round_trips() {
    use tessera_document::nodes::{Insets, TextLayout, VerticalJustify};

    let path = temp_path("columns.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let story = doc.add_story(tessera_text::story::Story::new("some copy"));
    let layer = doc.default_layer().expect("layer");
    let wanted = TextLayout {
        columns: 3,
        gutter: 18.0,
        inset: Insets {
            top: 4.0,
            bottom: 4.0,
            left: 6.0,
            right: 6.0,
        },
        vertical: VerticalJustify::Justify,
        lock_to_grid: true,
        next: None,
    };
    let id = doc.add_frame(
        layer,
        Frame {
            corners: tessera_document::corners::Corners::SQUARE,
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 200.0,
            },
            kind: FrameKind::Text {
                story,
                layout: wanted,
            },
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            style: None,
        },
    );

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");
    let FrameKind::Text { layout, .. } = loaded.frame(id).expect("frame").kind.clone() else {
        panic!("not a text frame");
    };

    assert_eq!(layout, wanted);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_version_eight_document_opens_with_no_masters_and_no_overrides() {
    // 8 -> 9 rewrites nothing, and for once the defaults really are the truth:
    // a document written before parent pages existed has no masters, none of
    // its pages is built on one, and nothing in it overrides anything. Every
    // one of those is what an empty collection means — unlike `layer_order` at
    // 7 -> 8, where empty meant a document that painted nothing.
    let path = temp_path("legacy_v8.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let layer = doc.default_layer().expect("layer");
    let id = doc.add_frame(
        layer,
        Frame {
            corners: tessera_document::corners::Corners::SQUARE,
            bounds: DocRect {
                x: 12.0,
                y: 14.0,
                width: 30.0,
                height: 20.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            style: None,
        },
    );

    format::save(&doc, &path).expect("save");
    format::rewrite_version_for_test(&path, 8).expect("stamp");

    let loaded = format::load(&path).expect("a version 8 document must still open");

    assert!(loaded.master_order.is_empty());
    assert!(loaded.overrides.is_empty());
    for page in loaded.page_ids() {
        assert_eq!(loaded.pages[page].master, None);
    }
    assert!(loaded.frame(id).is_some(), "and its contents came back");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_master_and_its_overrides_survive_a_round_trip() {
    let path = temp_path("masters.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    doc.setup.facing_pages = false;
    doc.reflow_spreads();
    let master = doc.add_master("A-Master");
    let on = doc.pages_of_master(master)[0];
    let bounds = doc.pages[on].bounds;
    let layer = doc.default_layer().expect("layer");
    let item = doc.add_frame(
        layer,
        Frame {
            corners: tessera_document::corners::Corners::SQUARE,
            bounds: DocRect {
                x: bounds.x + 10.0,
                y: bounds.y + 10.0,
                width: 40.0,
                height: 30.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            style: None,
        },
    );
    let page = doc.page_ids().next().expect("a page");
    doc.apply_master(page, Some(master));
    let local = doc.override_master_item(page, item).expect("an override");

    format::save(&doc, &path).expect("save");
    let loaded = format::load(&path).expect("load");

    assert_eq!(loaded.master_order.len(), 1);
    assert_eq!(
        loaded.masters[loaded.master_order[0]].name, "A-Master",
        "the master kept its name"
    );
    assert_eq!(
        loaded.pages[page].master,
        Some(on),
        "and the page still points at it"
    );
    assert_eq!(
        loaded.overrides.get(local).copied(),
        Some(item),
        "and the override still remembers what it stands in for"
    );

    let _ = std::fs::remove_file(&path);
}

/// A version-7 archive: two pages, each owning its own layer.
///
/// Written by hand, and it has to be. `rewrite_version_for_test` re-saves
/// through the *current* model, which has no per-page layers at all — the
/// fixture would carry `layer_order`, the migration would find nothing to
/// move, and the test would pass while testing nothing. Four fixtures in this
/// file were once built that way.
fn version_7_archive(path: &std::path::Path) -> serde_json::Value {
    use std::io::Write;

    // Start from a current document with two pages and a frame on each, then
    // rewrite its JSON into the shape version 7 wrote.
    let mut doc = Document::new();
    doc.setup.facing_pages = false;
    let second = doc.add_page();
    let first = doc.page_ids().next().expect("a page");
    let layer = doc.default_layer().expect("a layer");

    for page in [first, second] {
        let on = doc.pages[page].bounds;
        doc.add_frame(
            layer,
            Frame {
                corners: tessera_document::corners::Corners::SQUARE,
                bounds: DocRect {
                    x: on.x + 10.0,
                    y: on.y + 10.0,
                    width: 40.0,
                    height: 30.0,
                },
                kind: FrameKind::Rectangle,
                transform: Transform::IDENTITY,
                fill: Paint::Solid(Color::BLACK),
                stroke: None,
                wrap: tessera_document::nodes::TextWrap::None,
                blend: tessera_document::blending::Blending::PLAIN,
                shadow: None,
                style: None,
            },
        );
    }

    let mut value: serde_json::Value = serde_json::to_value(&doc).expect("to value");
    let frames: Vec<serde_json::Value> = value["layers"][1]["value"]["frames"]
        .as_array()
        .expect("frames")
        .clone();
    assert_eq!(frames.len(), 2, "one frame per page");

    // Two layers, one per page, each holding that page's frame — which is
    // exactly what the old model produced, and why the bug existed.
    value["layers"] = serde_json::json!([
        { "value": null, "version": 0 },
        {
            "value": { "frames": [frames[0]], "name": "Layer 1",
                       "visible": true, "locked": false },
            "version": 1
        },
        {
            "value": { "frames": [frames[1]], "name": "Layer 1",
                       "visible": true, "locked": false },
            "version": 1
        },
    ]);

    let pages = value["pages"].as_array_mut().expect("pages");
    let mut depth = 1;
    for page in pages.iter_mut() {
        if page["value"].is_null() {
            continue;
        }
        page["value"].as_object_mut().expect("page").insert(
            "layers".into(),
            serde_json::json!([{ "idx": depth, "version": 1 }]),
        );
        depth += 1;
    }

    // And version 7 knew nothing of either of these.
    let doc_map = value.as_object_mut().expect("document");
    doc_map.remove("layer_order");
    doc_map.remove("active_layer");

    let body = serde_json::to_vec(&value).expect("body");
    assert!(
        !String::from_utf8_lossy(&body).contains("layer_order"),
        "the fixture must genuinely lack the field"
    );

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("meta.json", options).expect("meta");
        zip.write_all(br#"{"format_version":7,"app_version":"0.1.0","created":"","modified":""}"#)
            .expect("meta body");
        zip.start_file("document.json", options).expect("doc");
        zip.write_all(&body).expect("doc body");
        zip.finish().expect("finish");
    }
    std::fs::write(path, buffer.into_inner()).expect("write fixture");
    value
}

#[test]
fn a_version_seven_documents_per_page_layers_merge_into_one() {
    let path = temp_path("legacy_v7_layers.tessera");
    let _ = std::fs::remove_file(&path);
    version_7_archive(&path);

    let loaded = format::load(&path).expect("a version 7 document must still open");

    assert_eq!(
        loaded.layer_ids().count(),
        1,
        "two pages that each had a layer come back with one between them"
    );
    let layer = loaded.default_layer().expect("a layer");
    assert_eq!(
        loaded.layers[layer].frames.len(),
        2,
        "and it holds what both pages held"
    );
    assert_eq!(loaded.layers[layer].name, "Layer 1");
    assert_eq!(loaded.active_layer, Some(layer), "and it is the active one");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_version_seven_documents_objects_stay_on_their_own_pages() {
    // The migration must not move anything. Each frame was drawn on a
    // different page and has to still be on it — which, now that a frame's
    // page is derived from where it sits, means the geometry survived.
    let path = temp_path("legacy_v7_pages.tessera");
    let _ = std::fs::remove_file(&path);
    version_7_archive(&path);

    let loaded = format::load(&path).expect("load");
    let pages: Vec<_> = loaded.page_ids().collect();
    assert_eq!(pages.len(), 2);

    for page in pages {
        assert_eq!(
            loaded.frames_on_page(page).len(),
            1,
            "one frame per page, as it was drawn"
        );
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_version_seven_document_that_paints_nothing_still_gets_a_layer() {
    // A document whose layer order came back empty would paint nothing at all
    // — the file would open blank. An absent `layer_order` defaults to empty,
    // so this is the case where the default is a lie.
    use std::io::Write;

    let path = temp_path("legacy_v7_bare.tessera");
    let _ = std::fs::remove_file(&path);

    let mut value: serde_json::Value = serde_json::to_value(Document::new()).expect("to value");
    // No layers anywhere: not a file the old writer produced, but the shape
    // the migration must not turn into a blank document.
    value["layers"] = serde_json::json!([{ "value": null, "version": 0 }]);
    for page in value["pages"].as_array_mut().expect("pages") {
        if let Some(page) = page["value"].as_object_mut() {
            page.insert("layers".into(), serde_json::json!([]));
        }
    }
    let doc_map = value.as_object_mut().expect("document");
    doc_map.remove("layer_order");
    doc_map.remove("active_layer");

    let body = serde_json::to_vec(&value).expect("body");
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("meta.json", options).expect("meta");
        zip.write_all(br#"{"format_version":7,"app_version":"0.1.0","created":"","modified":""}"#)
            .expect("meta body");
        zip.start_file("document.json", options).expect("doc");
        zip.write_all(&body).expect("doc body");
        zip.finish().expect("finish");
    }
    std::fs::write(&path, buffer.into_inner()).expect("write fixture");

    let loaded = format::load(&path).expect("load");
    assert_eq!(
        loaded.layer_ids().count(),
        1,
        "a document with nowhere to put an object is given somewhere"
    );

    let _ = std::fs::remove_file(&path);
}

/// Build a version-5 archive by hand and open it.
///
/// **`rewrite_version_for_test` cannot be used here.** It loads with the
/// current model and re-saves under an older stamp, so the JSON it produces
/// already carries `runs` — and the migration, which fires only on a story
/// that lacks them, skips it. Three tests written that way passed while
/// testing nothing at all. The fixture has to genuinely lack the field, which
/// is why `a_version_1_document_still_opens` builds one the same way.
fn version_5_archive(doc: &Document, path: &std::path::Path) {
    version_5_archive_styled(doc, path, 12.0, "sans-serif");
}

/// As above, with a chosen size and family in the version-5 `style`.
fn version_5_archive_styled(doc: &Document, path: &std::path::Path, size: f64, family: &str) {
    use std::io::Write;

    let style = serde_json::json!({
        "family": family,
        "size": size,
        "line_height": 1.2,
        "color": { "Rgb": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 } },
    });

    /// Make every story look as it did at version 5: no runs, and the single
    /// `style` the model has since dropped.
    ///
    /// Putting `style` back matters as much as taking `runs` away. The
    /// current `Story` has no such field, so a fixture built by serialising
    /// one would lack it — and the migration, which recognises a story by
    /// `text` and `style` together, would skip every one.
    fn make_version_5(value: &mut serde_json::Value, style: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if map.contains_key("text") && map.contains_key("runs") {
                    map.remove("runs");
                    map.remove("paragraphs");
                    map.insert("style".to_string(), style.clone());
                }
                for v in map.values_mut() {
                    make_version_5(v, style);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    make_version_5(v, style);
                }
            }
            _ => {}
        }
    }

    let mut value: serde_json::Value = serde_json::to_value(doc).expect("to value");
    make_version_5(&mut value, &style);
    let body = serde_json::to_vec(&value).expect("body");
    let text = String::from_utf8_lossy(&body);
    assert!(
        !text.contains("\"runs\""),
        "the fixture must genuinely lack the field"
    );
    assert!(
        !doc.stories.is_empty() && text.contains("\"style\""),
        "and must genuinely carry the one it had"
    );

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("meta.json", options).expect("meta");
        zip.write_all(br#"{"format_version":5,"app_version":"0.1.0","created":"","modified":""}"#)
            .expect("meta body");
        zip.start_file("document.json", options).expect("doc");
        zip.write_all(&body).expect("doc body");
        zip.finish().expect("finish");
    }
    std::fs::write(path, buffer.into_inner()).expect("write fixture");
}

#[test]
fn a_version_five_story_arrives_with_runs_that_describe_it() {
    // The first migration whose default was a lie. `runs` has serde(default)
    // and the default is an empty list — which for a story with text in it
    // satisfies no version of the run invariant. Without the rewrite, every
    // document saved before milestone 2 would open unsound.
    let path = temp_path("v5-story-runs.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let id = doc.add_story(tessera_text::story::Story::new("Hello, Tessera."));
    version_5_archive(&doc, &path);

    let loaded = format::load(&path).expect("a version-5 document must still open");
    let story = loaded.story(id).expect("the story survived");

    assert_eq!(story.text, "Hello, Tessera.");
    assert!(
        story.runs_are_sound(),
        "runs {:?} do not describe {:?}",
        story.runs,
        story.text
    );
    assert_eq!(story.runs.len(), 1, "one run covering the whole story");
    assert_eq!(story.paragraphs.len(), 1);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_migrated_run_keeps_the_formatting_the_story_already_had() {
    // The rewrite must not change how anything looks.
    let path = temp_path("v5-story-format.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let id = doc.add_story(tessera_text::story::Story::new("text"));
    version_5_archive_styled(&doc, &path, 18.5, "Georgia");

    let loaded = format::load(&path).expect("open");
    let run = &loaded.story(id).expect("story").runs[0];
    assert_eq!(run.local.size, Some(18.5));
    assert_eq!(run.local.family.as_deref(), Some("Georgia"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_empty_story_migrates_to_no_runs_rather_than_one_empty_run() {
    let path = temp_path("v5-empty-story.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let id = doc.add_story(tessera_text::story::Story::new(""));
    version_5_archive(&doc, &path);

    let loaded = format::load(&path).expect("open");
    let story = loaded.story(id).expect("story");
    assert!(story.runs.is_empty());
    assert!(story.runs_are_sound());

    let _ = std::fs::remove_file(&path);
}

/// Build a version-6 archive by hand: stories that have **both** `runs` and
/// the single `style` the model carried until version 7.
///
/// Version 6 is the one shape the current model cannot produce — it has runs
/// but no `style` — so the field has to be put back by hand. `runs` is left
/// exactly as the current model writes it, which is what makes the fixture a
/// genuine version 6 rather than a version 5 with extra keys.
fn version_6_archive(doc: &Document, path: &std::path::Path, size: f64, family: &str) {
    use std::io::Write;

    let style = serde_json::json!({
        "family": family,
        "size": size,
        "line_height": 1.2,
        "color": { "Rgb": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 } },
    });

    fn add_style(value: &mut serde_json::Value, style: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if map.contains_key("text") && map.contains_key("runs") {
                    map.insert("style".to_string(), style.clone());
                }
                for v in map.values_mut() {
                    add_style(v, style);
                }
            }
            serde_json::Value::Array(items) => {
                for v in items {
                    add_style(v, style);
                }
            }
            _ => {}
        }
    }

    let mut value: serde_json::Value = serde_json::to_value(doc).expect("to value");
    add_style(&mut value, &style);
    let body = serde_json::to_vec(&value).expect("body");
    let text = String::from_utf8_lossy(&body);
    assert!(
        !doc.stories.is_empty(),
        "a fixture with no story proves nothing"
    );
    assert!(
        text.contains("\"style\"") && text.contains("\"runs\""),
        "version 6 is the shape that has both; got {text}"
    );

    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("meta.json", options).expect("meta");
        zip.write_all(br#"{"format_version":6,"app_version":"0.1.0","created":"","modified":""}"#)
            .expect("meta body");
        zip.start_file("document.json", options).expect("doc");
        zip.write_all(&body).expect("doc body");
        zip.finish().expect("finish");
    }
    std::fs::write(path, buffer.into_inner()).expect("write fixture");
}

#[test]
fn a_version_six_story_folds_its_style_into_its_runs() {
    // Removing a field is only safe if nothing about the document changes. The
    // style said 18.5pt Georgia and the run said nothing, so after the fold
    // the run has to say 18.5pt Georgia — otherwise every document written
    // before version 7 reopens in the wrong face at the wrong size.
    let path = temp_path("v6-fold-style.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let id = doc.add_story(tessera_text::story::Story::new("text"));
    version_6_archive(&doc, &path, 18.5, "Georgia");

    let loaded = format::load(&path).expect("a version-6 document must still open");
    let story = loaded.story(id).expect("story");
    assert!(story.runs_are_sound());
    let run = &story.runs[0];
    assert_eq!(run.local.size, Some(18.5));
    assert_eq!(run.local.family.as_deref(), Some("Georgia"));
    assert_eq!(run.local.line_height, Some(1.2));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_run_that_already_stated_a_size_keeps_it_through_the_fold() {
    // The fold is `run.local` **over** the story style, not the other way
    // round. A run that had been given 9pt in the editor must stay 9pt, and
    // still pick up the family it never stated.
    let path = temp_path("v6-run-wins.tessera");
    let _ = std::fs::remove_file(&path);

    let mut doc = Document::new();
    let mut story = tessera_text::story::Story::new("text");
    story.runs[0].local.size = Some(9.0);
    let id = doc.add_story(story);
    version_6_archive(&doc, &path, 18.5, "Georgia");

    let loaded = format::load(&path).expect("open");
    let run = &loaded.story(id).expect("story").runs[0];
    assert_eq!(run.local.size, Some(9.0), "the run's own size wins");
    assert_eq!(
        run.local.family.as_deref(),
        Some("Georgia"),
        "and the style still fills what the run left unsaid"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn the_fold_survives_being_saved_again() {
    // A migration that patches only the in-memory document would look right
    // once and be lost on the next save. What proves the fold landed in the
    // model is the *re-saved* file: the run must carry 18.5pt Georgia itself,
    // with no story style anywhere to supply it.
    //
    // Asserting only that `style` is absent would prove nothing — serde drops
    // unknown keys on load, so that holds whether the migration runs or not.
    let old = temp_path("v6-then-saved.tessera");
    let new = temp_path("v7-after-save.tessera");
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new);

    let mut doc = Document::new();
    doc.add_story(tessera_text::story::Story::new("text"));
    version_6_archive(&doc, &old, 18.5, "Georgia");

    let loaded = format::load(&old).expect("open");
    format::save(&loaded, &new).expect("save");

    let file = std::fs::File::open(&new).expect("open archive");
    let mut zip = zip::ZipArchive::new(file).expect("archive");
    let mut body = String::new();
    {
        use std::io::Read;
        zip.by_name("document.json")
            .expect("document")
            .read_to_string(&mut body)
            .expect("read");
    }
    assert!(
        body.contains("Georgia") && body.contains("18.5"),
        "the folded formatting must be in the file, not just in memory; got {body}"
    );
    assert!(
        !body.contains("\"style\":{\"family\""),
        "and the old field is gone"
    );

    let _ = std::fs::remove_file(&old);
    let _ = std::fs::remove_file(&new);
}

#[test]
fn runs_survive_a_round_trip() {
    use tessera_text::story::{CharacterFormat, Run};

    let path = temp_path("runs-round-trip.tessera");
    let _ = std::fs::remove_file(&path);

    let mut original = Document::new();
    let mut story = tessera_text::story::Story::new("ab");
    story.runs = vec![
        Run {
            range: 0..1,
            style: None,
            local: CharacterFormat {
                weight: Some(700),
                ..CharacterFormat::default()
            },
        },
        Run::plain(1..2),
    ];
    let id = original.add_story(story);

    format::save(&original, &path).expect("save");
    let reopened = format::load(&path).expect("open");
    let back = reopened.story(id).expect("story");

    assert_eq!(back.runs.len(), 2, "the two runs came back");
    assert_eq!(back.runs[0].local.weight, Some(700));
    assert!(back.runs_are_sound());

    let _ = std::fs::remove_file(&path);
}
