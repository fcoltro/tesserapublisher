//! An IDML package built by hand, the way InDesign writes one, read back as
//! a document.
//!
//! No real export is checked in — InDesign is not on the machine that runs
//! this — so the package here is what the IDML specification and its
//! cookbook say an export looks like. The hand check against a real file is
//! owed and recorded in the roadmap.

use std::io::Write;
use std::path::Path;

use tessera_document::nodes::FrameKind;
use tessera_import::idml;
use tessera_text::variables::Marker;

fn package(entries: &[(&str, String)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("mimetype", zip::write::SimpleFileOptions::default())
        .expect("entry");
    zip.write_all(b"application/vnd.adobe.indesign-idml-package")
        .expect("write");
    for (name, text) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .expect("entry");
        zip.write_all(text.as_bytes()).expect("write");
    }
    zip.finish().expect("finish").into_inner()
}

const IDPKG: &str = r#"xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging""#;

/// A rectangle path, four corners, as InDesign writes it.
fn rect_path(x: f64, y: f64, w: f64, h: f64) -> String {
    let point = |px: f64, py: f64| {
        format!(
            r#"<PathPointType Anchor="{px} {py}" LeftDirection="{px} {py}" RightDirection="{px} {py}"/>"#
        )
    };
    format!(
        "<Properties><PathGeometry><GeometryPathType PathOpen=\"false\"><PathPointArray>{}{}{}{}</PathPointArray></GeometryPathType></PathGeometry></Properties>",
        point(x, y),
        point(x, y + h),
        point(x + w, y + h),
        point(x + w, y)
    )
}

fn a_book() -> Vec<u8> {
    let designmap = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Document xmlns:idPkg="http://ns.adobe.com/AdobeInDesign/idml/1.0/packaging" DOMVersion="18.0" Self="d">
  <idPkg:Graphic src="Resources/Graphic.xml"/>
  <idPkg:Styles src="Resources/Styles.xml"/>
  <idPkg:Preferences src="Resources/Preferences.xml"/>
  <Layer Self="ub3" Name="Layer 1" Visible="true" Locked="false"/>
  <Section Self="us1" Name="" PageStart="ub8" PageNumberStart="1" PageNumberStyle="LowerRoman" ContinueNumbering="false" SectionPrefix="" Marker="Front matter"/>
  <Section Self="us2" Name="" PageStart="uba" PageNumberStart="1" PageNumberStyle="Arabic" ContinueNumbering="false" SectionPrefix="" Marker=""/>
  <idPkg:MasterSpread src="MasterSpreads/MasterSpread_ub6.xml"/>
  <idPkg:Spread src="Spreads/Spread_ub7.xml"/>
  <idPkg:Spread src="Spreads/Spread_ub9.xml"/>
  <idPkg:Story src="Stories/Story_u12.xml"/>
  <idPkg:Story src="Stories/Story_u20.xml"/>
</Document>"#,
    );
    let graphic = format!(
        r#"<idPkg:Graphic {IDPKG}>
  <Color Self="Color/Black" Model="Process" Space="CMYK" ColorValue="0 0 0 100" Name="Black"/>
  <Color Self="Color/Brand red" Model="Process" Space="RGB" ColorValue="255 0 0" Name="Brand red"/>
  <Swatch Self="Swatch/None" Name="None"/>
  <Gradient Self="Gradient/Sunset" Name="Sunset" Type="Linear">
    <GradientStop Self="ug1" StopColor="Color/Brand red" Location="0"/>
    <GradientStop Self="ug2" StopColor="Color/Black" Location="100"/>
  </Gradient>
</idPkg:Graphic>"#
    );
    let styles = format!(
        r#"<idPkg:Styles {IDPKG}>
  <RootCharacterStyleGroup Self="u77">
    <CharacterStyle Self="CharacterStyle/$ID/[No character style]" Name="$ID/[No character style]"/>
    <CharacterStyle Self="CharacterStyle/Emphasis" Name="Emphasis" FontStyle="Italic"/>
  </RootCharacterStyleGroup>
  <RootParagraphStyleGroup Self="u78">
    <ParagraphStyle Self="ParagraphStyle/$ID/NormalParagraphStyle" Name="$ID/NormalParagraphStyle"/>
    <ParagraphStyle Self="ParagraphStyle/Body" Name="Body" PointSize="10" Justification="LeftJustified"><Properties><BasedOn type="object">ParagraphStyle/$ID/NormalParagraphStyle</BasedOn><Leading type="unit">13</Leading></Properties></ParagraphStyle>
    <ParagraphStyleGroup Self="u79" Name="Headings">
      <ParagraphStyle Self="ParagraphStyle/Headings%3aHeading" Name="Heading" PointSize="24" Justification="CenterAlign" SpaceBefore="12" KeepWithNext="1"><Properties><BasedOn type="object">ParagraphStyle/Body</BasedOn><AppliedFont type="string">Helvetica</AppliedFont></Properties></ParagraphStyle>
    </ParagraphStyleGroup>
  </RootParagraphStyleGroup>
  <RootObjectStyleGroup Self="u80">
    <ObjectStyle Self="ObjectStyle/$ID/[None]" Name="$ID/[None]"/>
    <ObjectStyle Self="ObjectStyle/Callout" Name="Callout" FillColor="Color/Brand red" StrokeColor="Color/Black" StrokeWeight="1"><TransparencySetting><BlendingSetting Opacity="60" BlendMode="Normal"/></TransparencySetting></ObjectStyle>
  </RootObjectStyleGroup>
</idPkg:Styles>"#
    );
    let preferences = format!(
        r#"<idPkg:Preferences {IDPKG}>
  <DocumentPreference PageHeight="792" PageWidth="612" FacingPages="true" DocumentBleedTopOffset="9" DocumentBleedBottomOffset="9" DocumentBleedInsideOrLeftOffset="9" DocumentBleedOutsideOrRightOffset="9"/>
</idPkg:Preferences>"#
    );
    // The parent: one page (a single-page parent is legal), a folio frame.
    let master = format!(
        r#"<idPkg:MasterSpread {IDPKG}>
<MasterSpread Self="ub6" Name="A-Master" NamePrefix="A" BaseName="Master" PageCount="1">
  <Page Self="ubm" Name="A" GeometricBounds="0 0 792 612" ItemTransform="1 0 0 1 -306 -396"><MarginPreference Top="36" Left="54" Bottom="48" Right="36"/></Page>
  <TextFrame Self="uf1" ParentStory="u20" PreviousTextFrame="n" NextTextFrame="n" ItemLayer="ub3" ItemTransform="1 0 0 1 -252 340">{}</TextFrame>
</MasterSpread></idPkg:MasterSpread>"#,
        rect_path(0.0, 0.0, 200.0, 20.0)
    );
    // Spread one: a single recto, built on the parent, holding the first
    // frame of the body and a red rectangle.
    let spread1 = format!(
        r#"<idPkg:Spread {IDPKG}>
<Spread Self="ub7" PageCount="1">
  <Page Self="ub8" Name="1" AppliedMaster="ub6" GeometricBounds="0 0 792 612" ItemTransform="1 0 0 1 0 -396"><MarginPreference Top="36" Left="54" Bottom="48" Right="36" ColumnCount="1"/></Page>
  <TextFrame Self="uf2" ParentStory="u12" PreviousTextFrame="n" NextTextFrame="uf3" ItemLayer="ub3" ItemTransform="1 0 0 1 54 -360"><TextFramePreference TextColumnCount="2" TextColumnGutter="12" VerticalJustification="TopAlign"/>{}</TextFrame>
  <Rectangle Self="ur1" FillColor="Color/Brand red" StrokeColor="Color/Black" StrokeWeight="2" ItemLayer="ub3" ItemTransform="1 0 0 1 100 200"><TextWrapPreference TextWrapMode="BoundingBoxTextWrap" TextWrapSide="BothSides"><Properties><TextWrapOffset Top="4" Left="4" Bottom="4" Right="4"/></Properties></TextWrapPreference>{}</Rectangle>
  <Rectangle Self="ur2" FillColor="Gradient/Sunset" GradientFillAngle="90" AppliedObjectStyle="ObjectStyle/Callout" ItemLayer="ub3" ItemTransform="1 0 0 1 300 200"><TransparencySetting><BlendingSetting Opacity="50" BlendMode="Multiply"/><DropShadowSetting Mode="Drop" Opacity="40" XOffset="3" YOffset="4" Size="6" EffectColor="Color/Black"/></TransparencySetting>{}</Rectangle>
</Spread></idPkg:Spread>"#,
        rect_path(0.0, 0.0, 300.0, 400.0),
        rect_path(0.0, 0.0, 100.0, 50.0),
        rect_path(0.0, 0.0, 80.0, 80.0)
    );
    // Spread two: verso and recto, the body continuing on the verso.
    let spread2 = format!(
        r#"<idPkg:Spread {IDPKG}>
<Spread Self="ub9" PageCount="2">
  <Page Self="uba" Name="2" AppliedMaster="ub6" GeometricBounds="0 0 792 612" ItemTransform="1 0 0 1 -612 -396"/>
  <Page Self="ubb" Name="3" AppliedMaster="n" GeometricBounds="0 0 792 612" ItemTransform="1 0 0 1 0 -396"/>
  <TextFrame Self="uf3" ParentStory="u12" PreviousTextFrame="uf2" NextTextFrame="n" ItemLayer="ub3" ItemTransform="1 0 0 1 -558 -360">{}</TextFrame>
</Spread></idPkg:Spread>"#,
        rect_path(0.0, 0.0, 300.0, 400.0)
    );
    let body = format!(
        r#"<idPkg:Story {IDPKG}>
<Story Self="u12">
  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Headings%3aHeading">
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>Chapter One</Content><Br/></CharacterStyleRange>
  </ParagraphStyleRange>
  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Body">
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content>It was a </Content></CharacterStyleRange>
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/Emphasis"><Content>bright</Content></CharacterStyleRange>
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]"><Content> cold day.</Content>
      <Footnote><ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Body"><CharacterStyleRange><Content><?ACE 4?>	Orwell.</Content></CharacterStyleRange></ParagraphStyleRange></Footnote><Br/>
    </CharacterStyleRange>
  </ParagraphStyleRange>
  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Body">
    <CharacterStyleRange>
      <Content>A picture </Content>
      <Rectangle Self="ur9" FillColor="Color/Black" ItemTransform="1 0 0 1 0 0">{}</Rectangle>
      <Content> and a table </Content>
      <Table Self="ut1" HeaderRowCount="0" BodyRowCount="2" ColumnCount="2">
        <Row Self="ut1r0" Name="0" SingleRowHeight="14"/>
        <Row Self="ut1r1" Name="1" SingleRowHeight="14"/>
        <Column Self="ut1c0" Name="0" SingleColumnWidth="60"/>
        <Column Self="ut1c1" Name="1" SingleColumnWidth="90"/>
        <Cell Self="ut1i0" Name="0:0" RowSpan="1" ColumnSpan="2"><ParagraphStyleRange><CharacterStyleRange><Content>Wide head</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
        <Cell Self="ut1i1" Name="0:1" RowSpan="1" ColumnSpan="1"><ParagraphStyleRange><CharacterStyleRange><Content>a</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
        <Cell Self="ut1i2" Name="1:1" RowSpan="1" ColumnSpan="1"><ParagraphStyleRange><CharacterStyleRange><Content>b</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
      </Table>
      <Content> follow.</Content>
    </CharacterStyleRange>
  </ParagraphStyleRange>
</Story></idPkg:Story>"#,
        rect_path(0.0, 0.0, 30.0, 20.0)
    );
    let folio = format!(
        r#"<idPkg:Story {IDPKG}>
<Story Self="u20">
  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Body" Justification="RightAlign">
    <CharacterStyleRange><Content><?ACE 19?> · <?ACE 18?></Content></CharacterStyleRange>
  </ParagraphStyleRange>
</Story></idPkg:Story>"#
    );
    package(&[
        ("designmap.xml", designmap),
        ("Resources/Graphic.xml", graphic),
        ("Resources/Styles.xml", styles),
        ("Resources/Preferences.xml", preferences),
        ("MasterSpreads/MasterSpread_ub6.xml", master),
        ("Spreads/Spread_ub7.xml", spread1),
        ("Spreads/Spread_ub9.xml", spread2),
        ("Stories/Story_u12.xml", body),
        ("Stories/Story_u20.xml", folio),
    ])
}

#[test]
fn a_book_comes_back_as_pages_parents_frames_threads_styles_and_sections() {
    let imported = idml::import_bytes(a_book(), Path::new("book.idml")).expect("import");
    let doc = &imported.document;

    // Pages and setup.
    let pages: Vec<_> = doc.page_ids().collect();
    assert_eq!(pages.len(), 3, "one recto, then a verso and a recto");
    assert!(doc.setup.facing_pages);
    let bounds = doc.pages[pages[0]].bounds;
    assert_eq!((bounds.width, bounds.height), (612.0, 792.0));
    assert_eq!(doc.setup.bleed.top, 9.0);
    assert_eq!(doc.setup.margins.top, 36.0);
    assert_eq!(doc.setup.margins.inside, 54.0);

    // Sections: front matter in roman, the body from page two.
    assert_eq!(doc.page_label(pages[0]).as_deref(), Some("i"));
    assert_eq!(doc.page_label(pages[1]).as_deref(), Some("1"));
    assert_eq!(doc.page_label(pages[2]).as_deref(), Some("2"));
    assert_eq!(
        doc.page_number(pages[0]).map(|n| n.marker).as_deref(),
        Some("Front matter")
    );

    // The parent, applied to pages one and two but not three.
    let master = doc.master_ids().next().expect("a parent");
    assert_eq!(doc.masters[master].name, "A-Master");
    let parent_pages = doc.pages_of_master(master);
    // Each on the parent page for its own side of the fold.
    assert!(
        doc.pages[pages[0]]
            .master
            .is_some_and(|m| parent_pages.contains(&m))
    );
    assert!(
        doc.pages[pages[1]]
            .master
            .is_some_and(|m| parent_pages.contains(&m))
    );
    assert_eq!(doc.pages[pages[2]].master, None);

    // Styles, with the group's style based on Body.
    let heading = doc
        .paragraph_styles
        .iter()
        .find(|(_, s)| s.name == "Heading")
        .map(|(id, s)| (id, s.clone()))
        .expect("Heading");
    let body = doc
        .paragraph_styles
        .iter()
        .find(|(_, s)| s.name == "Body")
        .map(|(id, _)| id)
        .expect("Body");
    assert_eq!(heading.1.based_on, Some(body));
    assert_eq!(heading.1.format.character.size, Some(24.0));
    assert_eq!(
        heading.1.format.character.family.as_deref(),
        Some("Helvetica")
    );
    assert!(heading.1.format.keep.is_some_and(|k| k.with_next));
    let body_style = &doc.paragraph_styles[body];
    assert_eq!(body_style.format.character.line_height, Some(1.3));
    assert!(
        doc.character_styles
            .iter()
            .any(|(_, s)| s.name == "Emphasis")
    );

    // Swatches, minus InDesign's own.
    assert!(doc.swatch("Brand red").is_some());
    assert!(doc.swatch("Black").is_some());

    // Frames: the body thread across two pages, the rectangle, the folio.
    let frames: Vec<_> = doc.paint_order();
    let text_frames: Vec<_> = frames
        .iter()
        .filter(|f| {
            matches!(
                doc.frame(**f).map(|f| &f.kind),
                Some(FrameKind::Text { .. })
            )
        })
        .copied()
        .collect();
    let first = text_frames
        .iter()
        .find(|f| doc.page_of_frame(**f) == Some(pages[0]))
        .copied()
        .expect("body frame on page one");
    let second = text_frames
        .iter()
        .find(|f| doc.page_of_frame(**f) == Some(pages[1]))
        .copied()
        .expect("body frame on page two");
    assert_eq!(
        doc.thread_of(first),
        vec![first, second],
        "threaded in order"
    );
    let FrameKind::Text { story, layout } = &doc.frame(first).unwrap().kind else {
        panic!()
    };
    assert_eq!(layout.columns, 2);
    assert_eq!(layout.gutter, 12.0);
    let text = doc.story(*story).unwrap();
    let m = tessera_document::anchored::MARKER;
    assert_eq!(
        text.text,
        format!(
            "Chapter One\nIt was a bright cold day.{}\nA picture {m} and a table {m} follow.",
            Marker::FootnoteReference.character()
        )
    );

    // The picture and the table are frames anchored to the two markers.
    let anchors = doc.anchors_in(*story);
    assert!(anchors.are_sound(2), "one frame per marker");
    let picture = doc.frame(anchors.frame_at(0).unwrap()).unwrap();
    assert!(matches!(picture.kind, FrameKind::Rectangle));
    assert_eq!((picture.bounds.width, picture.bounds.height), (30.0, 20.0));
    let table = doc.frame(anchors.frame_at(1).unwrap()).unwrap();
    let FrameKind::Table(table) = &table.kind else {
        panic!("a table frame")
    };
    assert_eq!((table.rows(), table.columns()), (2, 2));
    assert_eq!(table.columns, vec![60.0, 90.0]);
    assert!(table.spans_are_sound());
    let head = table.at(0, 0).unwrap().cell().expect("a cell");
    assert_eq!(head.span.columns, 2);
    assert_eq!(doc.story(head.story).unwrap().text, "Wide head");
    assert!(
        table.at(0, 1).unwrap().cell().is_none(),
        "covered by the span"
    );
    let b = table.at(1, 1).unwrap().cell().expect("a cell");
    assert_eq!(doc.story(b.story).unwrap().text, "b");
    assert!(text.notes_are_sound());
    assert!(text.footnotes[0].text.ends_with("Orwell."));
    assert_eq!(text.paragraphs[0].style, Some(heading.0));
    assert!(
        text.runs.iter().any(|r| r.style.is_some()),
        "Emphasis is applied"
    );

    // Placed relative to its page: 54 in from the page's left, 36 down.
    let frame = doc.frame(first).unwrap();
    let page = doc.pages[pages[0]].bounds;
    assert!(
        (frame.bounds.x - (page.x + 54.0)).abs() < 1e-6,
        "{}",
        frame.bounds.x
    );
    assert!(
        (frame.bounds.y - (page.y + 36.0)).abs() < 1e-6,
        "{}",
        frame.bounds.y
    );
    // And the verso frame relative to the verso.
    let frame = doc.frame(second).unwrap();
    let page = doc.pages[pages[1]].bounds;
    assert!(
        (frame.bounds.x - (page.x + 54.0)).abs() < 1e-6,
        "{}",
        frame.bounds.x
    );

    let rect = frames
        .iter()
        .find(|f| matches!(doc.frame(**f).map(|f| &f.kind), Some(FrameKind::Rectangle)))
        .expect("the rectangle");
    let rect = doc.frame(*rect).unwrap();
    assert!(
        matches!(&rect.fill, tessera_document::paint::Paint::Solid(tessera_color::Color::Rgb { r, .. }) if (*r - 1.0).abs() < 1e-6)
    );
    assert_eq!(rect.stroke.as_ref().map(|s| s.width), Some(2.0));
    assert_eq!((rect.bounds.width, rect.bounds.height), (100.0, 50.0));
    // Its wrap, with the side the text may run on.
    assert!(
        matches!(
            rect.wrap,
            tessera_document::nodes::TextWrap::Bounds {
                sides: tessera_document::nodes::WrapTo::Both,
                ..
            }
        ),
        "{:?}",
        rect.wrap
    );

    // The folio, on the parent, carrying both markers.
    let on_master = doc.pages_of_master(master)[0];
    let folio = text_frames
        .iter()
        .find(|f| doc.page_of_frame(**f) == Some(on_master))
        .expect("the folio frame");
    let FrameKind::Text { story, .. } = &doc.frame(*folio).unwrap().kind else {
        panic!()
    };
    assert_eq!(
        doc.story(*story).unwrap().text,
        format!(
            "{} · {}",
            Marker::SectionMarker.character(),
            Marker::PageNumber.character()
        )
    );

    assert!(imported.dropped.is_empty(), "{:?}", imported.dropped);
}

#[test]
fn a_gradient_fill_effects_and_an_object_style_come_through() {
    use tessera_document::blending::BlendMode;
    use tessera_document::paint::{Paint, Ramp};
    let imported = idml::import_bytes(a_book(), Path::new("book.idml")).expect("import");
    let doc = &imported.document;

    // The object style, with what it states.
    let (style_id, style) = doc
        .object_styles
        .iter()
        .find(|(_, s)| s.name == "Callout")
        .expect("the Callout style");
    assert!(
        !doc.object_styles
            .values()
            .any(|s| s.name.contains("[None]")),
        "InDesign's root is not a style"
    );
    assert!(matches!(style.format.fill, Some(Paint::Solid(_))));
    assert_eq!(
        style
            .format
            .stroke
            .as_ref()
            .and_then(|s| s.as_ref())
            .map(|s| s.width),
        Some(1.0)
    );
    assert!((style.format.blend.expect("stated").opacity - 0.6).abs() < 1e-6);

    // The rectangle wearing it: a gradient turned as InDesign turns it,
    // half-opaque multiply, a soft shadow, and the style attached.
    let frame = doc
        .paint_order()
        .into_iter()
        .filter_map(|id| doc.frame(id))
        .find(|f| matches!(&f.fill, Paint::Gradient(_)))
        .expect("the gradient-filled rectangle");
    let Paint::Gradient(gradient) = &frame.fill else {
        unreachable!()
    };
    assert_eq!(gradient.ramp, Ramp::Linear { angle: -90.0 });
    assert_eq!(gradient.stops().len(), 2);
    assert!((frame.blend.opacity - 0.5).abs() < 1e-6);
    assert_eq!(frame.blend.mode, BlendMode::Multiply);
    let shadow = frame.shadow.as_ref().expect("a drop shadow");
    assert_eq!(shadow.offset, (3.0, 4.0));
    assert_eq!(shadow.blur, 6.0);
    assert!(
        (shadow.colour.to_rgb_f32()[3] - 0.4).abs() < 1e-6,
        "its opacity in the alpha"
    );
    assert_eq!(frame.style, Some(style_id));

    // The plain red rectangle beside it has none of that.
    let plain = doc
        .paint_order()
        .into_iter()
        .filter_map(|id| doc.frame(id))
        .find(|f| f.stroke.as_ref().is_some_and(|s| s.width == 2.0))
        .expect("the red rectangle");
    assert!(plain.shadow.is_none());
    assert_eq!(plain.style, None);
}

#[test]
fn a_package_without_a_spine_is_refused_by_name() {
    let bytes = package(&[("Resources/Graphic.xml", "<a/>".into())]);
    let err = idml::import_bytes(bytes, Path::new("x.idml")).unwrap_err();
    assert!(matches!(err, tessera_import::ImportError::Missing(ref e) if e == "designmap.xml"));
}
