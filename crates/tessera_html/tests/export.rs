//! A document out as a web page, read back as text.

use tessera_document::contents::Level;
use tessera_document::document::Document;
use tessera_document::ids::StoryId;
use tessera_document::nodes::{Frame, FrameKind};
use tessera_geometry::{DocRect, Transform};
use tessera_text::story::{
    CharacterFormat, CharacterStyle, CrossReference, CrossReferenceFormat, Hyperlink,
    ParagraphStyle, Story, TextAnchor,
};
use tessera_text::variables::Marker;

/// A text frame at `(x, y)` on the first page, showing `story`.
fn frame_at(doc: &mut Document, x: f64, y: f64, story: StoryId) {
    let page = doc.page_ids().next().expect("a page");
    let b = doc.pages[page].bounds;
    let layer = doc.default_layer().expect("a layer");
    doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: b.x + x,
                y: b.y + y,
                width: 200.0,
                height: 100.0,
            },
            kind: FrameKind::text(story),
            transform: Transform::IDENTITY,
            fill: tessera_document::paint::Paint::Solid(tessera_color::Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            anchor: None,
            style: None,
            hidden: false,
            locked: false,
        },
    );
}

#[test]
fn stories_go_out_in_reading_order_with_their_styles_links_and_notes() {
    let mut doc = Document::new();
    let heading = doc.add_paragraph_style(ParagraphStyle {
        name: "Chapter head".into(),
        ..Default::default()
    });
    let strong = doc.character_styles.insert(CharacterStyle {
        name: "Strong".into(),
        based_on: None,
        format: CharacterFormat {
            weight: Some(700),
            ..Default::default()
        },
    });
    doc.contents.levels = vec![Level {
        style: heading,
        entry_style: None,
    }];

    // Lower on the page, but first in the story order: reading order is
    // where things are, not when they were made.
    let mut body = Story::new(format!(
        "A bold claim{}, see {} & <more>.",
        Marker::FootnoteReference.character(),
        Marker::CrossReference.character()
    ));
    body.footnotes = vec![{
        let mut note = Story::new_footnote();
        note.insert_text(note.text.len(), "Proved elsewhere.");
        note
    }];
    body.cross_references[0] = CrossReference {
        target: "intro".into(),
        format: CrossReferenceFormat::PageNumber,
    };
    // "bold": a link, and set in Strong.
    body.apply_character_format(
        2..6,
        &CharacterFormat {
            link: Some(Hyperlink::Url("https://example.com/".into())),
            ..Default::default()
        },
    );
    body.set_character_style(2..6, Some(strong));
    let body = doc.add_story(body);

    let mut title = Story::new(format!("{}Introduction", Marker::TextAnchor.character()));
    title.anchors[0] = TextAnchor {
        name: "intro".into(),
    };
    title.set_paragraph_style(0..title.text.len(), Some(heading));
    let title = doc.add_story(title);

    frame_at(&mut doc, 20.0, 300.0, body);
    frame_at(&mut doc, 20.0, 20.0, title);

    let out = tessera_html::export(&doc, &Default::default());
    let html = &out.html;
    let title_at = html.find("Introduction").expect("the heading");
    let body_at = html.find("A ").expect("the body");
    assert!(
        title_at < body_at,
        "the heading, above it on the page, first"
    );

    assert!(
        html.contains("<h1 class=\"p-chapter-head\"><a id=\"a-intro\"></a>Introduction</h1>"),
        "a contents style is a heading, and its anchor an id:\n{html}"
    );
    assert!(
        html.contains("<a href=\"https://example.com/\"><span class=\"c-strong\">bold</span></a>"),
        "a link round a styled run:\n{html}"
    );
    assert!(
        html.contains("<sup class=\"footnote-ref\"><a href=\"#fn-1\" id=\"fnref-1\">1</a></sup>"),
        "{html}"
    );
    assert!(html.contains("<li id=\"fn-1\" value=\"1\">"), "{html}");
    assert!(html.contains("Proved elsewhere."), "{html}");
    assert!(
        html.contains("<a href=\"#a-intro\">Introduction</a>"),
        "a cross-reference reads as its paragraph and links to it:\n{html}"
    );
    assert!(html.contains("&amp; &lt;more&gt;"), "escaped");
    assert!(html.contains(&format!("href=\"{}\"", tessera_html::STYLESHEET)));

    assert!(out.css.contains(".p-chapter-head"), "{}", out.css);
    assert!(
        out.css.contains(".c-strong {\n  font-weight: 700;\n}"),
        "{}",
        out.css
    );

    let inline = tessera_html::export(
        &doc,
        &tessera_html::Options {
            inline_css: true,
            ..Default::default()
        },
    );
    assert!(inline.css.is_empty() && inline.html.contains("<style>"));
}
