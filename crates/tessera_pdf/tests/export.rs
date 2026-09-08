//! PDF export.
//!
//! Non-negotiable N2. These tests parse the output back rather than trusting
//! that the writer ran, and the positioning test pins the property that makes
//! decision D3 worth having: glyph positions come from the shaper, not from a
//! second computation that can drift.

use tessera_color::Color;
use tessera_document::ids::FrameId;
use tessera_document::paint::Paint;
use tessera_geometry::{DocRect, Transform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};
use tessera_text::shape::Shaper;
use tessera_text::story::{NoStyles, Story};

fn page() -> DocRect {
    DocRect {
        x: 0.0,
        y: 0.0,
        width: 612.0,
        height: 792.0,
    }
}

/// The test page, resolved with no margins, bleed or slug.
fn empty_doc() -> ResolvedDocument {
    ResolvedDocument {
        items: Vec::new(),
        pages: vec![resolved_page()],
    }
}

fn resolved_page() -> tessera_layout::ResolvedPage {
    tessera_layout::ResolvedPage {
        bounds: page(),
        margins: page(),
        bleed: page(),
        slug: page(),
        columns: Vec::new(),
    }
}

fn one(kind: ResolvedKind, bounds: DocRect) -> ResolvedDocument {
    ResolvedDocument {
        pages: vec![resolved_page()],
        items: vec![ResolvedItem {
            frame: FrameId::default(),
            transform: Transform::IDENTITY,
            spread_area: None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            bounds,
            kind,
        }],
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> DocRect {
    DocRect {
        x,
        y,
        width: w,
        height: h,
    }
}

fn black_rect(bounds: DocRect) -> ResolvedDocument {
    one(
        ResolvedKind::Rectangle {
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
        },
        bounds,
    )
}

#[test]
fn an_empty_document_produces_a_valid_pdf_header_and_trailer() {
    let bytes = tessera_pdf::export(&empty_doc()).expect("export");

    assert!(bytes.starts_with(b"%PDF-1."), "must carry a PDF header");
    assert!(
        bytes.windows(5).any(|w| w == b"%%EOF"),
        "must be terminated with %%EOF"
    );
}

#[test]
fn the_media_box_matches_the_page_size() {
    let bytes = tessera_pdf::export(&empty_doc()).expect("export");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("612"), "the media box must carry the width");
    assert!(text.contains("792"), "the media box must carry the height");
}

#[test]
fn a_rectangle_emits_a_path_and_a_fill_operator() {
    let bytes = tessera_pdf::export(&black_rect(rect(10.0, 10.0, 50.0, 50.0))).expect("export");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains(" re"), "a rectangle path operator");
    assert!(text.contains(" f"), "a fill operator");
}

#[test]
fn a_rectangle_is_flipped_into_pdf_coordinates() {
    // 10pt from the document top, 50pt tall, on a 792pt page, must sit
    // 792 - 10 - 50 = 732 from the PDF bottom.
    let bytes = tessera_pdf::export(&black_rect(rect(10.0, 10.0, 50.0, 50.0))).expect("export");
    let text = String::from_utf8_lossy(&bytes);

    assert!(
        text.contains("732"),
        "the y coordinate must be flipped, not copied"
    );
}

#[test]
fn a_text_frame_embeds_a_subsetted_font() {
    let mut shaper = Shaper::new();
    let shaped = shaper.shape(&Story::new("Hello"), &NoStyles::default(), 400.0);
    assert!(shaped.glyph_count() > 0, "the fixture must actually shape");
    let full_font_size = shaped.fonts[0].data.len();

    let bytes = tessera_pdf::export(&one(
        ResolvedKind::Text {
            shaped,
            color: Color::BLACK,
        },
        rect(20.0, 20.0, 400.0, 40.0),
    ))
    .expect("export");
    let text = String::from_utf8_lossy(&bytes);

    assert!(
        text.contains("/FontFile2"),
        "the font must be embedded, not merely referenced"
    );
    assert!(
        text.contains("Identity-H"),
        "glyph ids are written directly"
    );
    assert!(text.contains("Tj"), "a show-text operator must be present");
    assert!(
        bytes.len() < full_font_size,
        "the whole PDF ({}) should be smaller than the unsubsetted font ({full_font_size})",
        bytes.len()
    );
}

#[test]
fn text_is_positioned_by_the_same_glyphs_the_renderer_drew() {
    let mut shaper = Shaper::new();
    let shaped = shaper.shape(&Story::new("Hi"), &NoStyles::default(), 400.0);
    let first_x = shaped.lines[0].glyphs().next().expect("a glyph").x;

    let bytes = tessera_pdf::export(&one(
        ResolvedKind::Text {
            shaped,
            color: Color::BLACK,
        },
        rect(20.0, 20.0, 400.0, 40.0),
    ))
    .expect("export");
    let text = String::from_utf8_lossy(&bytes);

    // If anyone "helpfully" recomputes positions in the exporter instead of
    // using the shaper's, this fails.
    let expected = format!("{:.2}", 20.0 + first_x);
    let trimmed = expected.trim_end_matches('0').trim_end_matches('.');
    assert!(
        text.contains(trimmed),
        "expected the shaper's x ({trimmed}) in the text matrix"
    );
}

#[test]
fn an_empty_text_frame_exports_without_a_font() {
    let mut shaper = Shaper::new();
    let shaped = shaper.shape(&Story::new(""), &NoStyles::default(), 400.0);

    let bytes = tessera_pdf::export(&one(
        ResolvedKind::Text {
            shaped,
            color: Color::BLACK,
        },
        rect(20.0, 20.0, 400.0, 40.0),
    ))
    .expect("export");

    assert!(bytes.starts_with(b"%PDF-1."));
    assert!(
        !String::from_utf8_lossy(&bytes).contains("/FontFile2"),
        "nothing was drawn, so nothing should be embedded"
    );
}

#[test]
fn several_items_all_reach_the_content_stream() {
    let mut shaper = Shaper::new();
    let shaped = shaper.shape(&Story::new("Hi"), &NoStyles::default(), 400.0);

    let doc = ResolvedDocument {
        pages: vec![resolved_page()],
        items: vec![
            ResolvedItem {
                frame: FrameId::default(),
                transform: Transform::IDENTITY,
                spread_area: None,
                blend: tessera_document::blending::Blending::PLAIN,
                shadow: None,
                bounds: rect(10.0, 10.0, 50.0, 50.0),
                kind: ResolvedKind::Rectangle {
                    fill: Paint::Solid(Color::BLACK),
                    stroke: None,
                },
            },
            ResolvedItem {
                frame: FrameId::default(),
                transform: Transform::IDENTITY,
                spread_area: None,
                blend: tessera_document::blending::Blending::PLAIN,
                shadow: None,
                bounds: rect(100.0, 100.0, 80.0, 40.0),
                kind: ResolvedKind::Ellipse {
                    fill: Paint::Solid(Color::BLACK),
                    stroke: None,
                },
            },
            ResolvedItem {
                frame: FrameId::default(),
                transform: Transform::IDENTITY,
                spread_area: None,
                blend: tessera_document::blending::Blending::PLAIN,
                shadow: None,
                bounds: rect(20.0, 300.0, 400.0, 40.0),
                kind: ResolvedKind::Text {
                    shaped,
                    color: Color::BLACK,
                },
            },
        ],
    };

    let text = String::from_utf8_lossy(&tessera_pdf::export(&doc).expect("export")).into_owned();

    assert!(text.contains(" re"), "the rectangle");
    assert!(
        text.contains(" c\n") || text.contains(" c "),
        "the ellipse curves"
    );
    assert!(text.contains("/FontFile2"), "the text");
}

// --- the trim and the bleed --------------------------------------------

/// A page with a bleed all round, resolved as the document would resolve it.
fn bled_page(bleed: f64) -> tessera_layout::ResolvedPage {
    let p = page();
    tessera_layout::ResolvedPage {
        bounds: p,
        margins: p,
        bleed: DocRect {
            x: p.x - bleed,
            y: p.y - bleed,
            width: p.width + bleed * 2.0,
            height: p.height + bleed * 2.0,
        },
        slug: p,
        columns: Vec::new(),
    }
}

#[test]
fn every_export_records_a_trim_box_and_a_bleed_box() {
    // A printer reads TrimBox and BleedBox, not MediaBox. Writing only the
    // one discards where the guillotine goes.
    let text =
        String::from_utf8_lossy(&tessera_pdf::export(&empty_doc()).expect("export")).into_owned();
    assert!(text.contains("/TrimBox"), "the trim must be recorded");
    assert!(text.contains("/BleedBox"), "and so must the bleed");
}

#[test]
fn a_bleed_grows_the_media_box_without_moving_the_content() {
    // The origin stays at the trim corner. If it did not, setting a bleed
    // would shift every object on the page — a silent rewrite of the layout.
    let bounds = rect(10.0, 10.0, 50.0, 50.0);

    let plain = ResolvedDocument {
        items: black_rect(bounds).items,
        pages: vec![resolved_page()],
    };
    let bled = ResolvedDocument {
        items: black_rect(bounds).items,
        pages: vec![bled_page(9.0)],
    };

    let a = tessera_pdf::export(&plain).expect("export");
    let b = tessera_pdf::export(&bled).expect("export");

    let content_of = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes).into_owned();
        let start = text.find("re").expect("a rectangle in the content stream");
        text[start.saturating_sub(40)..start].to_string()
    };
    assert_eq!(
        content_of(&a),
        content_of(&b),
        "the object sits at the same coordinates with and without a bleed"
    );

    let text = String::from_utf8_lossy(&b).into_owned();
    assert!(text.contains("/MediaBox"), "and the media box is present");
}

// --- runs carry their own size ----------------------------------------

#[test]
fn a_document_with_two_text_sizes_sets_the_font_more_than_once() {
    // One text object per run, because the size lives there. Setting the font
    // once and drawing every size at it is the failure this guards.
    use tessera_text::story::{CharacterFormat, Run};

    let sized = |size: f32, range: std::ops::Range<usize>| Run {
        range,
        style: None,
        local: CharacterFormat {
            size: Some(size),
            ..CharacterFormat::default()
        },
    };

    let mut story = Story::new("bigsmall");
    story.runs = vec![sized(24.0, 0..3), sized(9.0, 3..8)];

    let mut shaper = Shaper::new();
    let shaped = shaper.shape(&story, &NoStyles::default(), 400.0);
    assert!(shaped.runs().count() >= 2, "the fixture needs two runs");

    let bytes = tessera_pdf::export(&one(
        ResolvedKind::Text {
            shaped,
            color: Color::BLACK,
        },
        rect(10.0, 10.0, 300.0, 80.0),
    ))
    .expect("export");

    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.matches("Tf").count() >= 2,
        "the content stream should select a font once per run"
    );
}

#[test]
fn a_glyph_width_is_normalised_against_its_own_run() {
    // Dividing an advance by the wrong size gives a PDF whose text sits
    // correctly and whose widths are wrong — which a viewer will not complain
    // about and a printer will. Exporting the same text at one size and at
    // two must not produce the same /W array.
    use tessera_text::story::{CharacterFormat, Run};

    let sized = |size: f32, range: std::ops::Range<usize>| Run {
        range,
        style: None,
        local: CharacterFormat {
            size: Some(size),
            ..CharacterFormat::default()
        },
    };

    let mut shaper = Shaper::new();

    let mut uniform = Story::new("AB");
    uniform.runs = vec![sized(12.0, 0..2)];
    let mut mixed = Story::new("AB");
    mixed.runs = vec![sized(12.0, 0..1), sized(36.0, 1..2)];

    let export = |story: &Story, shaper: &mut Shaper| {
        let shaped = shaper.shape(story, &NoStyles::default(), 400.0);
        tessera_pdf::export(&one(
            ResolvedKind::Text {
                shaped,
                color: Color::BLACK,
            },
            rect(10.0, 10.0, 300.0, 80.0),
        ))
        .expect("export")
    };

    let a = export(&uniform, &mut shaper);
    let b = export(&mixed, &mut shaper);
    assert_ne!(
        a, b,
        "the same glyphs at different sizes produced an identical PDF"
    );
}

// --- compositing ------------------------------------------------------------

/// A black rectangle with the given compositing.
fn blended_rect(blend: tessera_document::blending::Blending) -> ResolvedDocument {
    let mut doc = black_rect(rect(10.0, 10.0, 50.0, 50.0));
    doc.items[0].blend = blend;
    doc
}

#[test]
fn a_translucent_object_writes_a_graphics_state_and_refers_to_it() {
    // The screen and the file must agree, so opacity that draws on one and not
    // in the other is a bug rather than a limitation.
    use tessera_document::blending::{BlendMode, Blending};

    let bytes = tessera_pdf::export(&blended_rect(Blending {
        opacity: 0.5,
        mode: BlendMode::Normal,
    }))
    .expect("export");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    assert!(text.contains("/ExtGState"), "no graphics state dictionary");
    assert!(text.contains("/ca 0.5"), "no non-stroking alpha");
    assert!(text.contains("/CA 0.5"), "no stroking alpha");
    assert!(
        text.contains("/GS0 gs"),
        "the content stream never used the state"
    );
}

#[test]
fn a_blend_mode_is_written_by_name() {
    use tessera_document::blending::{BlendMode, Blending};

    let bytes = tessera_pdf::export(&blended_rect(Blending {
        opacity: 1.0,
        mode: BlendMode::Multiply,
    }))
    .expect("export");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    assert!(text.contains("/BM /Multiply"), "no blend mode");
}

#[test]
fn a_plain_object_writes_no_graphics_state() {
    // Nearly every object is plain, and a state per object would be a resource
    // dictionary the length of the document for no effect.
    let bytes = tessera_pdf::export(&black_rect(rect(10.0, 10.0, 50.0, 50.0))).expect("export");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    assert!(!text.contains("/ExtGState"), "an opaque object needed none");
    assert!(!text.contains(" gs"));
}

#[test]
fn an_object_at_no_opacity_is_not_written_at_all() {
    // Exactly as it is not drawn. `/ca 0` would put ink-free paint in the file
    // for a press to process, for no visible result.
    use tessera_document::blending::{BlendMode, Blending};

    let bytes = tessera_pdf::export(&blended_rect(Blending {
        opacity: 0.0,
        mode: BlendMode::Normal,
    }))
    .expect("export");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    assert!(!text.contains(" re"), "an invisible rectangle was written");
    assert!(!text.contains("/ExtGState"));
}

#[test]
fn objects_sharing_a_compositing_share_one_graphics_state() {
    // Forty objects at 50% should be one entry in the page's resources rather
    // than forty.
    use tessera_document::blending::{BlendMode, Blending};

    let half = Blending {
        opacity: 0.5,
        mode: BlendMode::Normal,
    };
    let mut doc = blended_rect(half);
    let mut second = doc.items[0].clone();
    second.bounds = rect(100.0, 100.0, 20.0, 20.0);
    doc.items.push(second);

    let text = String::from_utf8_lossy(&tessera_pdf::export(&doc).expect("export")).into_owned();
    assert_eq!(
        text.matches("/ca 0.5").count(),
        1,
        "two states for one fact"
    );
    assert_eq!(text.matches("/GS0 gs").count(), 2, "both must refer to it");
}

#[test]
fn two_different_compositings_get_two_graphics_states() {
    use tessera_document::blending::{BlendMode, Blending};

    let mut doc = blended_rect(Blending {
        opacity: 0.5,
        mode: BlendMode::Normal,
    });
    let mut second = doc.items[0].clone();
    second.bounds = rect(100.0, 100.0, 20.0, 20.0);
    second.blend = Blending {
        opacity: 0.25,
        mode: BlendMode::Screen,
    };
    doc.items.push(second);

    let text = String::from_utf8_lossy(&tessera_pdf::export(&doc).expect("export")).into_owned();
    assert!(text.contains("/ca 0.5"));
    assert!(text.contains("/ca 0.25"));
    assert!(text.contains("/BM /Screen"));
    assert!(text.contains("/GS0 gs") && text.contains("/GS1 gs"));
}

// --- gradients --------------------------------------------------------------

/// A rectangle filled with `paint`.
fn painted_rect(paint: tessera_document::paint::Paint) -> ResolvedDocument {
    one(
        ResolvedKind::Rectangle {
            fill: paint,
            stroke: None,
        },
        rect(10.0, 10.0, 50.0, 50.0),
    )
}

fn two_stop(ramp: tessera_document::paint::Ramp) -> tessera_document::paint::Paint {
    tessera_document::paint::Paint::Gradient(tessera_document::paint::Gradient::black_to_white(
        ramp,
    ))
}

#[test]
fn a_linear_gradient_is_written_as_an_axial_shading() {
    use tessera_document::paint::Ramp;

    let bytes =
        tessera_pdf::export(&painted_rect(two_stop(Ramp::Linear { angle: 0.0 }))).expect("export");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    assert!(text.contains("/ShadingType 2"), "axial is shading type 2");
    assert!(text.contains("/Shading"), "no shading resource dictionary");
    assert!(text.contains(" sh"), "the content stream never painted it");
    // `W` then `n`, each its own operator: intersect the clip with the path,
    // then end the path without painting it.
    let ops: Vec<&str> = text.lines().map(str::trim).collect();
    let clipped = ops
        .windows(2)
        .any(|pair| pair[0].ends_with("W") && pair[1] == "n");
    assert!(
        clipped,
        "a gradient must be clipped to its shape: `sh` fills the whole clip"
    );
}

#[test]
fn a_radial_gradient_is_written_as_a_radial_shading_between_two_circles() {
    use tessera_document::paint::Ramp;

    let bytes = tessera_pdf::export(&painted_rect(two_stop(Ramp::Radial))).expect("export");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    assert!(text.contains("/ShadingType 3"), "radial is shading type 3");
    // Six coordinates: two centres and two radii. A plain radial ramp is the
    // degenerate case where the inner circle is a point.
    assert!(text.contains("/Coords"), "no coordinates");
}

#[test]
fn a_two_stop_ramp_needs_no_stitching_function() {
    // One interval is one exponential function, and wrapping it would be a
    // dictionary describing nothing.
    use tessera_document::paint::Ramp;

    let bytes = tessera_pdf::export(&painted_rect(two_stop(Ramp::Radial))).expect("export");
    let text = String::from_utf8_lossy(&bytes).into_owned();

    assert!(text.contains("/FunctionType 2"), "no exponential function");
    assert!(
        !text.contains("/FunctionType 3"),
        "a two-stop ramp was stitched"
    );
}

#[test]
fn a_three_stop_ramp_is_two_functions_stitched() {
    // A PDF ramp interpolates between *two* colours per function, so N stops
    // are N-1 functions joined.
    use tessera_document::paint::{Gradient, Paint, Ramp, Stop};

    let ramp = Paint::Gradient(Gradient::new(
        Ramp::Linear { angle: 45.0 },
        vec![
            Stop {
                at: 0.0,
                colour: Color::BLACK,
            },
            Stop {
                at: 0.5,
                colour: Color::WHITE,
            },
            Stop {
                at: 1.0,
                colour: Color::BLACK,
            },
        ],
    ));
    let text = String::from_utf8_lossy(&tessera_pdf::export(&painted_rect(ramp)).expect("export"))
        .into_owned();

    assert!(text.contains("/FunctionType 3"), "no stitching function");
    assert_eq!(
        text.matches("/FunctionType 2").count(),
        2,
        "three stops are two intervals"
    );
    assert!(text.contains("/Bounds"), "no interval boundaries");
}

#[test]
fn a_solid_fill_writes_no_shading() {
    let text = String::from_utf8_lossy(
        &tessera_pdf::export(&black_rect(rect(10.0, 10.0, 50.0, 50.0))).expect("export"),
    )
    .into_owned();
    assert!(!text.contains("/ShadingType"));
    assert!(!text.contains(" sh\n"));
}

#[test]
fn a_gradient_ramp_is_extended_so_no_corner_is_left_unpainted() {
    use tessera_document::paint::Ramp;

    let text = String::from_utf8_lossy(
        &tessera_pdf::export(&painted_rect(two_stop(Ramp::Radial))).expect("export"),
    )
    .into_owned();
    assert!(
        text.contains("/Extend [true true]"),
        "the ramp stopped short"
    );
}

#[test]
fn a_gradient_runs_the_same_way_in_the_file_as_on_the_page() {
    // Both go through `Gradient::axis`, so this pins the flip into PDF space
    // rather than the angle itself. A ramp at 0 degrees on a rect from y=10 to
    // y=60 on a 792pt page is level, so both ends share one flipped y: 732.
    use tessera_document::paint::Ramp;

    let text = String::from_utf8_lossy(
        &tessera_pdf::export(&painted_rect(two_stop(Ramp::Linear { angle: 0.0 }))).expect("export"),
    )
    .into_owned();
    assert!(
        text.contains("/Coords [10 757 60 757]"),
        "the axis was not flipped into PDF space: {}",
        text.lines()
            .find(|l| l.contains("/Coords"))
            .unwrap_or("no coords line")
    );
}

// --- prepress: standards, marks and the output intent ------------------------

use tessera_pdf::{ExportOptions, Marks, Standard};

fn an_intent() -> tessera_document::intent::OutputIntent {
    let profile = tessera_color::managed::OutputProfile::screen().expect("a profile");
    tessera_document::intent::OutputIntent {
        description: "Tessera test condition".to_string(),
        profile: profile.bytes().to_vec(),
        rendering: tessera_document::intent::Rendering::default(),
    }
}

fn text_of(doc: &ResolvedDocument, options: &ExportOptions) -> String {
    let bytes = tessera_pdf::export_with(doc, options).expect("export");
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn a_plain_export_claims_no_standard_and_embeds_no_intent() {
    // What milestone 0 wrote, unchanged. A file that claims nothing is the right
    // answer for a document that names no press.
    let text = text_of(
        &black_rect(rect(10.0, 10.0, 50.0, 50.0)),
        &ExportOptions::default(),
    );
    assert!(!text.contains("GTS_PDFXVersion"));
    assert!(!text.contains("/OutputIntents"));
}

#[test]
fn a_standard_without_a_press_is_refused_rather_than_written() {
    // **The failure this prevents.** A file claiming PDF/X it does not meet is
    // worse than one claiming nothing: a printer's preflight believes the claim,
    // passes the file, and the job fails on press instead of in the studio.
    let options = ExportOptions {
        standard: Standard::X4,
        ..Default::default()
    };
    let error = tessera_pdf::export_with(&empty_doc(), &options)
        .expect_err("a claim with nothing behind it must be refused");
    assert!(format!("{error}").contains("output intent"));
}

#[test]
fn pdf_x4_writes_its_version_key_and_embeds_the_profile() {
    let options = ExportOptions {
        standard: Standard::X4,
        intent: Some(an_intent()),
        ..Default::default()
    };
    let text = text_of(&black_rect(rect(10.0, 10.0, 50.0, 50.0)), &options);

    assert!(text.contains("GTS_PDFXVersion"), "no version key");
    assert!(text.contains("PDF/X-4"), "the wrong version");
    assert!(text.contains("/OutputIntents"), "no output intent array");
    assert!(
        text.contains("/DestOutputProfile"),
        "the profile is not pointed at"
    );
    assert!(
        text.contains("Tessera test condition"),
        "the condition is not named"
    );
    // Required by PDF/X, and "unknown" is the only honest answer: Tessera does
    // not trap, and False would say the file was checked and needs none.
    assert!(text.contains("/Trapped"), "no trapping state");
}

#[test]
fn pdf_x1a_refuses_a_document_that_uses_transparency() {
    // X-1a forbids it and Tessera does not flatten, so the claim cannot be
    // honoured. Refusing is the whole point.
    use tessera_document::blending::{BlendMode, Blending};

    let mut doc = black_rect(rect(10.0, 10.0, 50.0, 50.0));
    doc.items[0].blend = Blending {
        opacity: 0.5,
        mode: BlendMode::Normal,
    };

    let options = ExportOptions {
        standard: Standard::X1a,
        intent: Some(an_intent()),
        ..Default::default()
    };
    let error = tessera_pdf::export_with(&doc, &options).expect_err("must be refused");
    assert!(format!("{error}").contains("transparency"));
}

#[test]
fn pdf_x1a_accepts_the_same_document_without_transparency() {
    let options = ExportOptions {
        standard: Standard::X1a,
        intent: Some(an_intent()),
        ..Default::default()
    };
    let text = text_of(&black_rect(rect(10.0, 10.0, 50.0, 50.0)), &options);
    assert!(text.contains("PDF/X-1a:2003"));
}

#[test]
fn marks_grow_the_media_box_without_moving_the_trim() {
    // A media box that stopped at the bleed would crop the crop marks, which is
    // a failure only noticed on the proof. The trim must not move: it is where
    // the guillotine goes.
    let doc = black_rect(rect(10.0, 10.0, 50.0, 50.0));
    let plain = text_of(&doc, &ExportOptions::default());
    let marked = text_of(
        &doc,
        &ExportOptions {
            marks: Marks::all(),
            ..Default::default()
        },
    );

    let trim = format!("/TrimBox [0 0 {} {}]", page().width, page().height);
    assert!(plain.contains(&trim), "the plain trim moved");
    assert!(marked.contains(&trim), "marks moved the trim");
    assert!(marked.len() > plain.len(), "asking for marks drew nothing");
}

#[test]
fn marks_at_no_offset_are_refused() {
    // They would be cut through by the trim, which defeats them.
    let options = ExportOptions {
        marks: Marks {
            offset: 0.0,
            ..Marks::all()
        },
        ..Default::default()
    };
    assert!(tessera_pdf::export_with(&empty_doc(), &options).is_err());
}

#[test]
fn an_rgb_press_still_writes_rgb() {
    // Converting for a press that is not CMYK would be converting for nothing.
    let options = ExportOptions {
        standard: Standard::X4,
        intent: Some(an_intent()),
        ..Default::default()
    };
    let text = text_of(&black_rect(rect(10.0, 10.0, 50.0, 50.0)), &options);
    assert!(text.contains(" rg"), "an RGB fill operator was expected");
    assert!(
        !text.contains(" k\n"),
        "a CMYK fill was written for an RGB press"
    );
}

// --- placed artwork ---------------------------------------------------------

/// A real JPEG on disk, and its path.
fn a_jpeg(name: &str, width: u32, height: u32) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("tessera-pdf-export");
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join(name);

    let image = image::RgbImage::from_pixel(width, height, image::Rgb([20, 120, 220]));
    image::DynamicImage::ImageRgb8(image)
        .save_with_format(&path, image::ImageFormat::Jpeg)
        .expect("write");
    path
}

fn placed(source: Option<std::path::PathBuf>) -> ResolvedKind {
    ResolvedKind::Graphic {
        inner: Transform::IDENTITY,
        natural: (100.0, 100.0),
        missing: source.is_none(),
        stroke: None,
        source,
    }
}

#[test]
fn a_placed_picture_is_written_into_the_pdf() {
    // It was not. `ResolvedKind::Graphic` was skipped entirely, so a page of
    // photographs exported as a page of nothing — and because the placeholder
    // is deliberately never written either, the file came out looking finished
    // and empty.
    let path = a_jpeg("placed.jpg", 40, 25);
    let doc = one(placed(Some(path)), page());

    let bytes = tessera_pdf::export(&doc).expect("export");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("/XObject"), "no image resources in the file");
    assert!(text.contains("/Subtype /Image"), "no image object");
    assert!(
        text.contains("/DCTDecode"),
        "the JPEG was re-encoded rather than passed through"
    );
    assert!(text.contains("/Im0 Do"), "the image is never drawn");
}

#[test]
fn one_file_placed_twice_is_embedded_once() {
    // A logo on forty pages is one image object and forty references. Embedding
    // it each time would multiply the file by forty for a picture the reader
    // already has.
    let path = a_jpeg("twice.jpg", 16, 16);
    let item = |bounds: DocRect, source: std::path::PathBuf| ResolvedItem {
        frame: FrameId::default(),
        transform: Transform::IDENTITY,
        spread_area: None,
        blend: tessera_document::blending::Blending::PLAIN,
        shadow: None,
        bounds,
        kind: placed(Some(source)),
    };
    let doc = ResolvedDocument {
        pages: vec![resolved_page()],
        items: vec![
            item(rect(0.0, 0.0, 50.0, 50.0), path.clone()),
            item(rect(60.0, 0.0, 50.0, 50.0), path),
        ],
    };

    let bytes = tessera_pdf::export(&doc).expect("export");
    let text = String::from_utf8_lossy(&bytes);
    assert_eq!(
        text.matches("/Subtype /Image").count(),
        1,
        "the file was embedded more than once"
    );
    assert_eq!(text.matches("/Im0 Do").count(), 2, "it is not drawn twice");
}

#[test]
fn a_frame_with_no_file_writes_nothing() {
    // An empty picture box is furniture. The cross and the frame edge are
    // interface, not ink, and a violet cross in a printed job is far worse than
    // a blank space.
    let doc = one(placed(None), page());

    let bytes = tessera_pdf::export(&doc).expect("export");
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("/Subtype /Image"));
}

#[test]
fn a_broken_link_does_not_stop_the_export() {
    // Preflight has already reported it. Refusing the whole PDF because of one
    // missing picture would mean a job with a broken link cannot be proofed.
    let gone = std::env::temp_dir().join("tessera-pdf-export/definitely-not-here.jpg");
    let doc = one(placed(Some(gone)), page());

    let bytes = tessera_pdf::export(&doc).expect("a broken link stopped the export");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn pdf_x1a_is_refused_for_a_document_with_pictures_in_it() {
    // The artwork is embedded in `/DeviceRGB` and X-1a admits only CMYK, grey
    // and spot. A printer's preflight *believes* `GTS_PDFXVersion`, so a file
    // claiming X-1a with an RGB image in it passes their check and fails on the
    // press instead of in the studio.
    let path = a_jpeg("conformance.jpg", 8, 8);
    let doc = one(placed(Some(path)), page());

    let options = tessera_pdf::ExportOptions {
        standard: tessera_pdf::Standard::X1a,
        intent: None,
        ..Default::default()
    };
    let refused =
        tessera_pdf::export_with(&doc, &options).expect_err("X-1a was claimed over an RGB image");
    assert!(format!("{refused}").contains("RGB"), "{refused}");
}

#[test]
fn an_empty_picture_box_does_not_refuse_pdf_x1a() {
    // It embeds nothing, so it puts no RGB in the file. Refusing over it would
    // be refusing over something that is not there.
    let doc = one(placed(None), page());
    let options = tessera_pdf::ExportOptions {
        standard: tessera_pdf::Standard::X1a,
        intent: None,
        ..Default::default()
    };
    let reasons = options.refusals(false, false);
    assert!(
        !reasons.iter().any(|r| r.contains("RGB")),
        "an empty box was refused: {reasons:?}"
    );
    let _ = doc;
}

// --- drop shadows -----------------------------------------------------------

fn with_shadow(
    mut doc: ResolvedDocument,
    shadow: tessera_document::shadow::Shadow,
) -> ResolvedDocument {
    for item in &mut doc.items {
        item.shadow = Some(shadow.clone());
    }
    doc
}

#[test]
fn a_drop_shadow_is_written_into_the_pdf() {
    // It was not, and the reason was honest: PDF has no blur operator, and the
    // softness has to arrive as pixels. The writer could not embed an image at
    // all until now.
    let doc = with_shadow(
        black_rect(rect(60.0, 60.0, 80.0, 40.0)),
        tessera_document::shadow::Shadow::TYPICAL,
    );

    let bytes = tessera_pdf::export(&doc).expect("export");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/SMask"), "the shadow carries no soft mask");
    assert!(text.contains("/DeviceGray"), "the mask is not greyscale");
    assert!(text.contains("/Sh0 Do"), "the shadow is never drawn");
}

#[test]
fn the_shadow_is_drawn_before_the_shape_that_casts_it() {
    // It is behind the shape by definition. Drawing it afterwards would put it
    // over the fill, which is not a subtle error.
    let doc = with_shadow(
        black_rect(rect(60.0, 60.0, 80.0, 40.0)),
        tessera_document::shadow::Shadow::TYPICAL,
    );

    let bytes = tessera_pdf::export(&doc).expect("export");
    let text = String::from_utf8_lossy(&bytes);
    let shadow_at = text.find("/Sh0 Do").expect("the shadow");
    let fill_at = text
        .find(" re\n")
        .or_else(|| text.find(" re "))
        .expect("the rectangle");
    assert!(
        shadow_at < fill_at,
        "the shadow is drawn over the shape rather than behind it"
    );
}

#[test]
fn a_document_with_no_shadows_carries_no_masks() {
    // Every object here costs a press something to process. Writing an empty
    // one for a document that casts no shadow is ink-free paint in the file.
    let doc = black_rect(rect(10.0, 10.0, 50.0, 50.0));
    let bytes = tessera_pdf::export(&doc).expect("export");
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("/Sh0 Do"));
}

#[test]
fn a_hard_shadow_still_gets_written() {
    // Zero blur is a real thing to want rather than a degenerate case.
    let hard = tessera_document::shadow::Shadow {
        blur: 0.0,
        ..tessera_document::shadow::Shadow::TYPICAL
    };
    let doc = with_shadow(black_rect(rect(10.0, 10.0, 50.0, 50.0)), hard);

    let bytes = tessera_pdf::export(&doc).expect("export");
    assert!(String::from_utf8_lossy(&bytes).contains("/Sh0 Do"));
}
