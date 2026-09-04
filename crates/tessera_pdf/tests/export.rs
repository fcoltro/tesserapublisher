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
    let bytes = tessera_pdf::export(&ResolvedDocument::default(), page()).expect("export");

    assert!(bytes.starts_with(b"%PDF-1."), "must carry a PDF header");
    assert!(
        bytes.windows(5).any(|w| w == b"%%EOF"),
        "must be terminated with %%EOF"
    );
}

#[test]
fn the_media_box_matches_the_page_size() {
    let bytes = tessera_pdf::export(&ResolvedDocument::default(), page()).expect("export");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("612"), "the media box must carry the width");
    assert!(text.contains("792"), "the media box must carry the height");
}

#[test]
fn a_rectangle_emits_a_path_and_a_fill_operator() {
    let bytes =
        tessera_pdf::export(&black_rect(rect(10.0, 10.0, 50.0, 50.0)), page()).expect("export");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains(" re"), "a rectangle path operator");
    assert!(text.contains(" f"), "a fill operator");
}

#[test]
fn a_rectangle_is_flipped_into_pdf_coordinates() {
    // 10pt from the document top, 50pt tall, on a 792pt page, must sit
    // 792 - 10 - 50 = 732 from the PDF bottom.
    let bytes =
        tessera_pdf::export(&black_rect(rect(10.0, 10.0, 50.0, 50.0)), page()).expect("export");
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

    let bytes = tessera_pdf::export(
        &one(
            ResolvedKind::Text {
                shaped,
                color: Color::BLACK,
            },
            rect(20.0, 20.0, 400.0, 40.0),
        ),
        page(),
    )
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
    let first_x = shaped.lines[0].glyphs[0].x;

    let bytes = tessera_pdf::export(
        &one(
            ResolvedKind::Text {
                shaped,
                color: Color::BLACK,
            },
            rect(20.0, 20.0, 400.0, 40.0),
        ),
        page(),
    )
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

    let bytes = tessera_pdf::export(
        &one(
            ResolvedKind::Text {
                shaped,
                color: Color::BLACK,
            },
            rect(20.0, 20.0, 400.0, 40.0),
        ),
        page(),
    )
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

    let text =
        String::from_utf8_lossy(&tessera_pdf::export(&doc, page()).expect("export")).into_owned();

    assert!(text.contains(" re"), "the rectangle");
    assert!(
        text.contains(" c\n") || text.contains(" c "),
        "the ellipse curves"
    );
    assert!(text.contains("/FontFile2"), "the text");
}
