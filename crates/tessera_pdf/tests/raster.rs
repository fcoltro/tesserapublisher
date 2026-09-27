//! Pages as pictures: PNG and JPEG, read back rather than trusted.

use tessera_color::Color;
use tessera_document::ids::FrameId;
use tessera_document::nodes::Stroke;
use tessera_document::paint::Paint;
use tessera_geometry::{DocRect, Transform};
use tessera_layout::ResolvedPage;
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};
use tessera_pdf::raster::{self, Format, ImageOptions};

fn rect(x: f64, y: f64, width: f64, height: f64) -> DocRect {
    DocRect {
        x,
        y,
        width,
        height,
    }
}

/// A 200 by 100 point page, bled 10 points all round.
fn page() -> ResolvedPage {
    let trim = rect(0.0, 0.0, 200.0, 100.0);
    ResolvedPage {
        bounds: trim,
        margins: trim,
        bleed: rect(-10.0, -10.0, 220.0, 120.0),
        slug: trim,
        columns: Vec::new(),
    }
}

fn filled(frame: FrameId, bounds: DocRect, colour: Color) -> ResolvedItem {
    ResolvedItem {
        frame,
        on: None,
        links: Vec::new(),
        transform: Transform::IDENTITY,
        spread_area: None,
        blend: tessera_document::blending::Blending::PLAIN,
        shadow: None,
        bounds,
        kind: ResolvedKind::Rectangle {
            outline: None,
            fill: Paint::Solid(colour),
            stroke: None,
        },
    }
}

fn document(items: Vec<ResolvedItem>) -> ResolvedDocument {
    ResolvedDocument {
        bookmarks: Vec::new(),
        items,
        pages: vec![page()],
    }
}

/// A black box in the page's top left quarter.
fn a_black_box() -> ResolvedDocument {
    document(vec![filled(
        FrameId::default(),
        rect(0.0, 0.0, 100.0, 50.0),
        Color::BLACK,
    )])
}

fn one(doc: &ResolvedDocument, options: &ImageOptions) -> raster::PageImage {
    let mut pages = raster::page_images(doc, options).expect("pictures");
    assert_eq!(pages.len(), 1);
    pages.remove(0)
}

fn decoded(image: &raster::PageImage) -> image::RgbaImage {
    image::load_from_memory(&image.bytes)
        .expect("reads back")
        .to_rgba8()
}

#[test]
fn a_page_is_a_png_as_many_pixels_as_its_resolution_asks() {
    for (ppi, size) in [(72.0, (200, 100)), (144.0, (400, 200)), (300.0, (833, 417))] {
        let options = ImageOptions {
            ppi,
            ..ImageOptions::default()
        };
        let image = one(&a_black_box(), &options);
        assert_eq!((image.width, image.height), size, "at {ppi} ppi");
        assert!(image.bytes.starts_with(b"\x89PNG"), "not a PNG");
        let pixels = decoded(&image);
        assert_eq!(pixels.dimensions(), size);
        // The box fills the top left quarter; the rest is paper.
        let (w, h) = size;
        assert_eq!(pixels.get_pixel(w / 4, h / 4).0, [0, 0, 0, 255]);
        assert_eq!(
            pixels.get_pixel(w * 3 / 4, h * 3 / 4).0,
            [255, 255, 255, 255]
        );
    }
}

#[test]
fn a_png_says_the_resolution_it_was_made_at() {
    // So a program placing it knows the size it was made for: 300 ppi is
    // 11,811 pixels a metre, which is how PNG counts.
    let image = one(
        &a_black_box(),
        &ImageOptions {
            ppi: 300.0,
            ..ImageOptions::default()
        },
    );
    let reader = png::Decoder::new(std::io::Cursor::new(&image.bytes))
        .read_info()
        .expect("a PNG");
    let dims = reader.info().pixel_dims.expect("a resolution");
    assert_eq!((dims.xppu, dims.yppu), (11811, 11811));
    assert_eq!(dims.unit, png::Unit::Meter);
    assert_eq!(
        reader.info().color_type,
        png::ColorType::Rgb,
        "on white, an alpha channel would be a quarter of the file for nothing"
    );
}

#[test]
fn a_transparent_png_leaves_the_paper_clear() {
    let image = one(
        &a_black_box(),
        &ImageOptions {
            ppi: 72.0,
            transparent: true,
            ..ImageOptions::default()
        },
    );
    let pixels = decoded(&image);
    assert_eq!(pixels.get_pixel(50, 25).0, [0, 0, 0, 255]);
    assert_eq!(pixels.get_pixel(150, 75).0[3], 0, "the paper was painted");
}

/// The JFIF header's density: its unit (1 is per inch) and the two values.
fn jfif_density(bytes: &[u8]) -> (u8, u16, u16) {
    let at = bytes
        .windows(5)
        .position(|w| w == b"JFIF\0")
        .expect("a JFIF header");
    let units = bytes[at + 7];
    let x = u16::from_be_bytes([bytes[at + 8], bytes[at + 9]]);
    let y = u16::from_be_bytes([bytes[at + 10], bytes[at + 11]]);
    (units, x, y)
}

#[test]
fn a_jpeg_is_on_white_and_says_its_resolution() {
    // Asked for clear paper, which a JPEG cannot have: it is white.
    let image = one(
        &a_black_box(),
        &ImageOptions {
            format: Format::Jpeg,
            ppi: 150.0,
            transparent: true,
            ..ImageOptions::default()
        },
    );
    assert!(image.bytes.starts_with(&[0xFF, 0xD8]), "not a JPEG");
    assert_eq!(jfif_density(&image.bytes), (1, 150, 150));
    let pixels = decoded(&image);
    let [r, g, b, _] = pixels.get_pixel(50, 25).0;
    assert!(r < 16 && g < 16 && b < 16, "the box is {r} {g} {b}");
    let [r, g, b, _] = pixels.get_pixel(300, 150).0;
    assert!(r > 240 && g > 240 && b > 240, "the paper is {r} {g} {b}");
}

#[test]
fn a_better_jpeg_is_a_bigger_file() {
    // A gradient, so there is detail for the quality to keep or lose.
    let mut doc = a_black_box();
    doc.items[0].bounds = rect(0.0, 0.0, 200.0, 100.0);
    doc.items[0].kind = ResolvedKind::Rectangle {
        outline: None,
        fill: Paint::Gradient(tessera_document::paint::Gradient::black_to_white(
            tessera_document::paint::Ramp::Linear { angle: 30.0 },
        )),
        stroke: None,
    };
    let size = |quality| {
        one(
            &doc,
            &ImageOptions {
                format: Format::Jpeg,
                ppi: 150.0,
                quality,
                ..ImageOptions::default()
            },
        )
        .bytes
        .len()
    };
    assert!(size(20) < size(60));
    assert!(size(60) < size(100));
}

#[test]
fn the_bleed_is_taken_in_only_when_asked() {
    // A box wholly in the bleed, off the left edge of the trim.
    let doc = document(vec![filled(
        FrameId::default(),
        rect(-10.0, 0.0, 10.0, 100.0),
        Color::BLACK,
    )]);
    let trimmed = one(
        &doc,
        &ImageOptions {
            ppi: 72.0,
            ..ImageOptions::default()
        },
    );
    assert_eq!((trimmed.width, trimmed.height), (200, 100));
    assert_eq!(
        decoded(&trimmed).get_pixel(0, 50).0,
        [255, 255, 255, 255],
        "the bleed showed on a trimmed page"
    );

    let bled = one(
        &doc,
        &ImageOptions {
            ppi: 72.0,
            bleed: true,
            ..ImageOptions::default()
        },
    );
    assert_eq!((bled.width, bled.height), (220, 120));
    assert_eq!(decoded(&bled).get_pixel(5, 60).0, [0, 0, 0, 255]);
    assert_eq!(decoded(&bled).get_pixel(15, 60).0, [255, 255, 255, 255]);
}

#[test]
fn every_page_is_a_picture_in_order() {
    let mut doc = a_black_box();
    let mut second = page();
    for r in [&mut second.bounds, &mut second.bleed, &mut second.margins] {
        r.y += 200.0;
    }
    doc.pages.push(second);
    // Red on the second page.
    doc.items.push(filled(
        FrameId::default(),
        rect(0.0, 200.0, 200.0, 100.0),
        Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
    ));
    let pages = raster::page_images(
        &doc,
        &ImageOptions {
            ppi: 72.0,
            ..ImageOptions::default()
        },
    )
    .expect("pictures");
    assert_eq!(pages.len(), 2);
    assert_eq!(
        decoded(&pages[0]).get_pixel(150, 75).0,
        [255, 255, 255, 255]
    );
    assert_eq!(decoded(&pages[1]).get_pixel(150, 75).0, [255, 0, 0, 255]);
}

#[test]
fn a_resolution_out_of_range_is_brought_into_it() {
    let at = |ppi| {
        let image = one(
            &a_black_box(),
            &ImageOptions {
                ppi,
                ..ImageOptions::default()
            },
        );
        (image.width, image.height)
    };
    assert_eq!(at(1.0), at(10.0), "a picture too coarse to read");
    assert_eq!(at(10.0), (28, 14));
}

#[test]
fn a_page_too_big_to_be_a_picture_says_so() {
    let mut small = a_black_box();
    small.pages[0].bounds = rect(0.0, 0.0, 20.0, 10.0);
    let fine = raster::page_images(
        &small,
        &ImageOptions {
            ppi: 2400.0,
            ..ImageOptions::default()
        },
    )
    .expect("20 points at 2400 ppi is 667 pixels");
    assert_eq!((fine[0].width, fine[0].height), (667, 333));

    let mut huge = a_black_box();
    huge.pages[0].bounds = rect(0.0, 0.0, 2000.0, 100.0);
    let error = raster::page_images(
        &huge,
        &ImageOptions {
            ppi: 2400.0,
            ..ImageOptions::default()
        },
    )
    .expect_err("66,667 pixels across");
    assert!(matches!(
        error,
        tessera_pdf::PdfError::TooLarge { width: 66667, .. }
    ));
}

fn ids(n: usize) -> Vec<FrameId> {
    let mut arena = slotmap::SlotMap::<FrameId, ()>::with_key();
    (0..n).map(|_| arena.insert(())).collect()
}

#[test]
fn a_selection_is_cut_out_to_what_it_paints() {
    let [a, b, c] = ids(3)[..] else {
        unreachable!()
    };
    let mut stroked = filled(a, rect(20.0, 20.0, 40.0, 20.0), Color::BLACK);
    let mut hairline = Stroke::new(Color::BLACK, 4.0);
    hairline.align = tessera_document::nodes::StrokeAlign::Outside;
    stroked.kind = ResolvedKind::Rectangle {
        outline: None,
        fill: Paint::Solid(Color::BLACK),
        stroke: Some(hairline),
    };
    let doc = document(vec![
        stroked,
        filled(b, rect(100.0, 60.0, 20.0, 20.0), Color::BLACK),
        filled(c, rect(150.0, 10.0, 10.0, 10.0), Color::BLACK),
    ]);

    // An outside stroke four points wide reaches four points past the box.
    let alone = raster::cut_out(&doc, &[a]).expect("something painted");
    assert_eq!(alone.pages[0].bounds, rect(16.0, 16.0, 48.0, 28.0));
    assert_eq!(alone.items.len(), 1, "only what was selected");

    let two = raster::cut_out(&doc, &[a, b]).expect("something painted");
    assert_eq!(two.pages[0].bounds, rect(16.0, 16.0, 104.0, 64.0));
    assert_eq!(two.items.len(), 2);

    let pictured = one(
        &two,
        &ImageOptions {
            ppi: 72.0,
            transparent: true,
            ..ImageOptions::default()
        },
    );
    assert_eq!((pictured.width, pictured.height), (104, 64));
    let pixels = decoded(&pictured);
    assert_eq!(pixels.get_pixel(1, 1).0[3], 255, "the stroke's corner");
    assert_eq!(
        pixels.get_pixel(103, 63).0[3],
        255,
        "the second box's corner"
    );
    assert_eq!(pixels.get_pixel(100, 5).0[3], 0, "nothing else is in it");

    assert!(raster::cut_out(&doc, &[]).is_none());
    // A frame with no size and nothing round it paints nothing to picture.
    let [d] = ids(1)[..] else { unreachable!() };
    let empty = document(vec![filled(d, rect(5.0, 5.0, 0.0, 0.0), Color::BLACK)]);
    assert!(raster::cut_out(&empty, &[d]).is_none());
}

#[test]
fn a_parent_page_object_is_cut_out_once() {
    // Resolved once for each page it shows on, a strip across every page
    // would be a picture of nothing anybody selected.
    let [a] = ids(1)[..] else { unreachable!() };
    let mut pages = slotmap::SlotMap::<tessera_document::ids::PageId, ()>::with_key();
    let (first, second) = (pages.insert(()), pages.insert(()));
    let mut on_first = filled(a, rect(10.0, 10.0, 20.0, 20.0), Color::BLACK);
    on_first.on = Some(first);
    let mut on_second = filled(a, rect(10.0, 210.0, 20.0, 20.0), Color::BLACK);
    on_second.on = Some(second);
    let cut = raster::cut_out(&document(vec![on_first, on_second]), &[a]).expect("painted");
    assert_eq!(cut.items.len(), 1);
    assert_eq!(cut.pages[0].bounds, rect(10.0, 10.0, 20.0, 20.0));
}

#[test]
fn a_turned_or_shadowed_selection_is_cut_out_whole() {
    let [a] = ids(1)[..] else { unreachable!() };
    let mut turned = filled(a, rect(0.0, 0.0, 20.0, 20.0), Color::BLACK);
    turned.transform =
        Transform::rotate_about(45.0, tessera_geometry::DocPoint { x: 10.0, y: 10.0 });
    let area = raster::cut_out(&document(vec![turned]), &[a])
        .expect("painted")
        .pages[0]
        .bounds;
    let diagonal = 20.0 * std::f64::consts::SQRT_2;
    assert!((area.width - diagonal).abs() < 1e-9, "{area:?}");
    assert!((area.x - (10.0 - diagonal / 2.0)).abs() < 1e-9, "{area:?}");

    let mut shadowed = filled(a, rect(0.0, 0.0, 20.0, 20.0), Color::BLACK);
    shadowed.shadow = Some(tessera_document::shadow::Shadow {
        offset: (6.0, 0.0),
        blur: 0.0,
        colour: Color::BLACK,
    });
    let area = raster::cut_out(&document(vec![shadowed]), &[a])
        .expect("painted")
        .pages[0]
        .bounds;
    assert_eq!(
        area,
        rect(0.0, 0.0, 26.0, 20.0),
        "out to where the shadow falls"
    );
}
