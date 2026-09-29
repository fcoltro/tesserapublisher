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

#[test]
fn a_picture_goes_out_as_its_frame_shows_it() {
    // A picture 20 by 10, red on its left half and blue on its right,
    // placed at its own size in a frame 10 by 10: the frame shows the red
    // half, and so does the page.
    let dir = std::env::temp_dir().join(format!("tessera-html-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("halves.png");
    image::RgbaImage::from_fn(20, 10, |x, _| {
        if x < 10 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 255])
        }
    })
    .save(&path)
    .unwrap();

    let mut doc = Document::new();
    let page = doc.page_ids().next().unwrap();
    let b = doc.pages[page].bounds;
    let layer = doc.default_layer().unwrap();
    let frame = doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: b.x + 20.0,
                y: b.y + 20.0,
                width: 10.0,
                height: 10.0,
            },
            kind: FrameKind::Graphic { placed: None },
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
    let link = doc.add_link(tessera_document::links::Link::new(&path, (20.0, 10.0)));
    doc.place(frame, link, tessera_document::graphic::Fit::Centre);
    // Centred, the picture's middle is the frame's; move it so its left
    // edge is the frame's, which shows the red half.
    if let FrameKind::Graphic { placed: Some(p) } = &mut doc.frames[frame].kind {
        p.inner = Transform::IDENTITY;
    }

    let out = tessera_html::export(
        &doc,
        &tessera_html::Options {
            ppi: 72.0,
            ..Default::default()
        },
    );
    assert_eq!(out.files.len(), 1, "one picture");
    let (name, bytes) = &out.files[0];
    assert!(
        name.starts_with("images/halves-") && name.ends_with(".jpg"),
        "{name}: opaque, so JPEG"
    );
    assert!(
        out.html.contains(&format!(
            "<img src=\"{name}\" alt=\"halves\" width=\"13\" height=\"13\">"
        )),
        "{}",
        out.html
    );
    let shown = image::load_from_memory(bytes).unwrap().to_rgb8();
    assert_eq!(shown.dimensions(), (10, 10), "the frame's size at 72 ppi");
    for p in shown.pixels() {
        assert!(p[0] > 200 && p[2] < 60, "the red half only: {p:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_table_goes_out_with_its_heading_row_spans_and_fills() {
    use tessera_document::table::{Slot, Span};
    let mut doc = Document::new();
    let mut texts = vec!["Name", "Size", "Wide", "", "a", "b"].into_iter();
    let mut table = tessera_document::table::new(3, 2, 200.0, || {
        doc.add_story(Story::new(texts.next().unwrap_or_default()))
    });
    table.header_rows = 1;
    // The second row's first cell spans both columns.
    if let Some(Slot::Cell(cell)) = table.at_mut(1, 0) {
        cell.span = Span {
            columns: 2,
            rows: 1,
        };
    }
    if let Some(slot) = table.at_mut(1, 1) {
        *slot = Slot::Covered;
    }
    table.alternating = Some(Box::new(
        tessera_document::table::AlternatingFills::every_other_row(
            tessera_document::paint::Paint::Solid(tessera_color::Color::Rgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
        ),
    ));
    let page = doc.page_ids().next().unwrap();
    let b = doc.pages[page].bounds;
    let layer = doc.default_layer().unwrap();
    doc.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: b.x + 20.0,
                y: b.y + 20.0,
                width: 200.0,
                height: 60.0,
            },
            kind: FrameKind::Table(table),
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

    let html = tessera_html::export(&doc, &Default::default()).html;
    let at = |s: &str| {
        html.find(s)
            .unwrap_or_else(|| panic!("{s} missing:\n{html}"))
    };
    assert!(
        at("<thead>") < at("<th ") && at("</thead>") < at("<tbody>"),
        "{html}"
    );
    assert!(html.contains(">Name</p></th>"), "{html}");
    assert!(html.contains("colspan=\"2\""), "{html}");
    assert_eq!(
        html.matches("<td").count(),
        3,
        "the covered slot is no cell: {html}"
    );
    // Body rows alternate: the first of them red, the next plain.
    let wide = &html[html.find("colspan").unwrap()..];
    assert!(
        wide.split("</tr>")
            .next()
            .unwrap()
            .contains("background: #ff0000"),
        "{html}"
    );
    let last_row = html.rsplit("<tr>").next().unwrap();
    assert!(!last_row.contains("background"), "{html}");
    assert!(html.contains("<col style=\"width: 100pt\">"), "{html}");
}
