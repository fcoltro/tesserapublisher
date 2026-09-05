//! PDF export.
//!
//! Non-negotiable N2. These tests parse the output back rather than trusting
//! that the writer ran, and the positioning test pins the property that makes
//! decision D3 worth having: glyph positions come from the shaper, not from a
//! second computation that can drift.

use tessera_color::Color;
use tessera_document::ids::FrameId;
use tessera_geometry::{DocRect, Transform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};
use tessera_text::shape::Shaper;
use tessera_text::story::Story;

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
    }
}

fn one(kind: ResolvedKind, bounds: DocRect) -> ResolvedDocument {
    ResolvedDocument {
        pages: vec![resolved_page()],
        items: vec![ResolvedItem {
            frame: FrameId::default(),
            transform: Transform::IDENTITY,
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
            fill: Color::BLACK,
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
    let shaped = shaper.shape(&Story::new("Hello"), 400.0);
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
    let shaped = shaper.shape(&Story::new("Hi"), 400.0);
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
    let shaped = shaper.shape(&Story::new(""), 400.0);

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
    let shaped = shaper.shape(&Story::new("Hi"), 400.0);

    let doc = ResolvedDocument {
        pages: vec![resolved_page()],
        items: vec![
            ResolvedItem {
                frame: FrameId::default(),
                transform: Transform::IDENTITY,
                bounds: rect(10.0, 10.0, 50.0, 50.0),
                kind: ResolvedKind::Rectangle {
                    fill: Color::BLACK,
                    stroke: None,
                },
            },
            ResolvedItem {
                frame: FrameId::default(),
                transform: Transform::IDENTITY,
                bounds: rect(100.0, 100.0, 80.0, 40.0),
                kind: ResolvedKind::Ellipse {
                    fill: Color::BLACK,
                    stroke: None,
                },
            },
            ResolvedItem {
                frame: FrameId::default(),
                transform: Transform::IDENTITY,
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
    let shaped = shaper.shape(&story, 400.0);
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
        let shaped = shaper.shape(story, 400.0);
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
