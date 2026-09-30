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
        feather: None,
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
    let mut pages = raster::page_images(doc, options, None).expect("pictures");
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
        None,
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
        None,
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
        None,
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

// --- formats, colours and profiles ------------------------------------------

use tessera_pdf::raster::Colour;

fn crpc6() -> tessera_document::intent::OutputIntent {
    let profile = include_bytes!("../../../assets/profiles/CGATS21_CRPC6.icc").to_vec();
    tessera_document::intent::OutputIntent {
        description: "CRPC6".to_string(),
        profile,
        rendering: tessera_document::intent::Rendering::default(),
    }
}

fn made(options: ImageOptions, press: Option<&tessera_document::intent::OutputIntent>) -> Vec<u8> {
    let mut pages = raster::page_images(&a_black_box(), &options, press).expect("pictures");
    pages.remove(0).bytes
}

fn at_72(format: Format, colour: Colour) -> ImageOptions {
    ImageOptions {
        format,
        colour,
        ppi: 72.0,
        ..ImageOptions::default()
    }
}

/// The value of a TIFF tag, read back with the same crate a reader would.
fn tiff_tag(bytes: &[u8], tag: tiff::tags::Tag) -> Option<tiff::decoder::ifd::Value> {
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(bytes)).expect("a TIFF");
    decoder.find_tag(tag).expect("readable")
}

#[test]
fn a_tiff_is_lossless_and_says_its_resolution() {
    let bytes = made(
        ImageOptions {
            ppi: 150.0,
            ..at_72(Format::Tiff, Colour::Rgb)
        },
        None,
    );
    assert!(
        bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*"),
        "not a TIFF"
    );
    let pixels = image::load_from_memory(&bytes).expect("reads").to_rgba8();
    assert_eq!(pixels.dimensions(), (417, 208));
    assert_eq!(pixels.get_pixel(100, 50).0, [0, 0, 0, 255]);
    assert_eq!(pixels.get_pixel(300, 150).0, [255, 255, 255, 255]);

    use tiff::decoder::ifd::Value;
    use tiff::tags::Tag;
    assert_eq!(
        tiff_tag(&bytes, Tag::Compression).and_then(|v| v.into_u16().ok()),
        Some(5),
        "LZW"
    );
    assert_eq!(
        tiff_tag(&bytes, Tag::Predictor).and_then(|v| v.into_u16().ok()),
        Some(2),
        "the horizontal predictor, which is what makes LZW pay on a photograph"
    );
    assert_eq!(
        tiff_tag(&bytes, Tag::ResolutionUnit).and_then(|v| v.into_u16().ok()),
        Some(2),
        "per inch"
    );
    assert!(matches!(
        tiff_tag(&bytes, Tag::XResolution),
        Some(Value::Rational(15000, 100))
    ));
    assert!(
        tiff_tag(&bytes, Tag::IccProfile).is_some(),
        "sRGB, as asked"
    );
}

#[test]
fn a_cmyk_tiff_is_in_the_press_s_inks_and_carries_its_profile() {
    let press = crpc6();
    let bytes = made(at_72(Format::Tiff, Colour::Cmyk), Some(&press));
    let mut decoder = tiff::decoder::Decoder::new(std::io::Cursor::new(&bytes)).expect("a TIFF");
    assert_eq!(
        decoder.colortype().expect("a colour type"),
        tiff::ColorType::CMYK(8)
    );
    let tiff::decoder::DecodingResult::U8(inks) = decoder.read_image().expect("pixels") else {
        panic!("eight bits a sample");
    };
    let at = |x: usize, y: usize| &inks[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4];
    assert_eq!(
        at(50, 25),
        [0, 0, 0, 255],
        "solid black is the black plate alone"
    );
    assert_eq!(at(150, 75), [0, 0, 0, 0], "and the paper is no ink");
    let profile = tiff_tag(&bytes, tiff::tags::Tag::IccProfile).expect("a profile");
    let tiff::decoder::ifd::Value::List(values) = profile else {
        panic!("the profile's bytes");
    };
    assert_eq!(values.len(), press.profile.len(), "the press's own profile");
}

#[test]
fn cmyk_without_a_cmyk_press_is_refused() {
    let refused = raster::page_images(&a_black_box(), &at_72(Format::Jpeg, Colour::Cmyk), None);
    assert!(matches!(
        refused,
        Err(tessera_pdf::PdfError::CannotConform(_))
    ));

    let screen = tessera_color::managed::OutputProfile::screen().expect("sRGB");
    let rgb_press = tessera_document::intent::OutputIntent {
        description: "sRGB".into(),
        profile: screen.bytes().to_vec(),
        rendering: tessera_document::intent::Rendering::default(),
    };
    let refused = raster::page_images(
        &a_black_box(),
        &at_72(Format::Jpeg, Colour::Cmyk),
        Some(&rgb_press),
    );
    assert!(
        matches!(refused, Err(tessera_pdf::PdfError::CannotConform(_))),
        "an RGB press makes no inks"
    );
}

/// The JPEG marker segments before the picture, as (marker, payload).
fn jpeg_segments(bytes: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        out.push((marker, &bytes[at + 4..at + 2 + length]));
        if marker == 0xDA {
            break;
        }
        at += 2 + length;
    }
    out
}

#[test]
fn a_cmyk_jpeg_is_adobe_s_kind_with_the_press_profile() {
    let press = crpc6();
    let bytes = made(at_72(Format::Jpeg, Colour::Cmyk), Some(&press));
    let segments = jpeg_segments(&bytes);
    assert!(
        segments
            .iter()
            .any(|(m, p)| *m == 0xEE && p.starts_with(b"Adobe")),
        "no Adobe marker, so no reader would take it as CMYK"
    );
    let icc: Vec<u8> = segments
        .iter()
        .filter(|(m, p)| *m == 0xE2 && p.starts_with(b"ICC_PROFILE\0"))
        .flat_map(|(_, p)| p[14..].to_vec())
        .collect();
    assert_eq!(icc, press.profile, "the press's profile, whole");
    let sof = segments
        .iter()
        .find(|(m, _)| *m == 0xC0)
        .expect("a baseline frame");
    assert_eq!(sof.1[5], 4, "four components");
}

#[test]
fn a_progressive_jpeg_is_one_and_a_profile_only_when_asked() {
    let progressive = made(
        ImageOptions {
            progressive: true,
            ..at_72(Format::Jpeg, Colour::Rgb)
        },
        None,
    );
    let markers: Vec<u8> = jpeg_segments(&progressive)
        .iter()
        .map(|(m, _)| *m)
        .collect();
    assert!(
        markers.contains(&0xC2),
        "no progressive frame in {markers:x?}"
    );
    assert!(markers.contains(&0xE2), "sRGB is embedded by default");

    let plain = made(
        ImageOptions {
            embed_profile: false,
            ..at_72(Format::Jpeg, Colour::Rgb)
        },
        None,
    );
    let markers: Vec<u8> = jpeg_segments(&plain).iter().map(|(m, _)| *m).collect();
    assert!(markers.contains(&0xC0), "baseline unless asked");
    assert!(!markers.contains(&0xE2), "a profile nobody asked for");
}

#[test]
fn a_grey_picture_is_one_channel_of_lightness() {
    // Pure red, whose lightness is about a fifth.
    let doc = document(vec![filled(
        FrameId::default(),
        rect(0.0, 0.0, 100.0, 50.0),
        Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        },
    )]);
    for format in [Format::Png, Format::Jpeg, Format::Tiff, Format::WebP] {
        let bytes = raster::page_images(&doc, &at_72(format, Colour::Grey), None)
            .expect("pictures")
            .remove(0)
            .bytes;
        let picture = image::load_from_memory(&bytes).expect("reads");
        if format == Format::WebP {
            // WebP has no grey of its own: three equal channels.
            let [r, g, b, _] = picture.to_rgba8().get_pixel(50, 25).0;
            assert!(r == g && g == b, "WebP grey is {r} {g} {b}");
        } else {
            assert!(
                matches!(picture.color(), image::ColorType::L8),
                "{format:?} came out {:?}",
                picture.color()
            );
        }
        let red = picture.to_luma8().get_pixel(50, 25).0[0];
        assert!((50..=58).contains(&red), "{format:?}: red is {red}");
    }
}

#[test]
fn a_png_says_it_is_srgb_rather_than_carry_a_profile() {
    let with = made(at_72(Format::Png, Colour::Rgb), None);
    assert!(with.windows(4).any(|w| w == b"sRGB"));
    assert!(!with.windows(4).any(|w| w == b"iCCP"));
    let without = made(
        ImageOptions {
            embed_profile: false,
            ..at_72(Format::Png, Colour::Rgb)
        },
        None,
    );
    assert!(!without.windows(4).any(|w| w == b"sRGB"));
}

#[test]
fn a_webp_is_lossless_and_can_leave_the_paper_clear() {
    let bytes = made(
        ImageOptions {
            transparent: true,
            ..at_72(Format::WebP, Colour::Rgb)
        },
        None,
    );
    assert!(
        bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        "not a WebP"
    );
    let pixels = image::load_from_memory(&bytes).expect("reads").to_rgba8();
    assert_eq!(
        pixels.get_pixel(50, 25).0,
        [0, 0, 0, 255],
        "lossless: exactly black"
    );
    assert_eq!(pixels.get_pixel(150, 75).0[3], 0, "the paper is clear");
    assert!(bytes.windows(4).any(|w| w == b"ICCP"), "sRGB, as asked");
}

#[test]
fn choices_a_file_cannot_hold_are_settled_before_it_is_made() {
    let settled = |format, colour, transparent| {
        ImageOptions {
            format,
            colour,
            transparent,
            ..ImageOptions::default()
        }
        .settled()
    };
    assert!(
        !settled(Format::Jpeg, Colour::Rgb, true).transparent,
        "JPEG has no alpha"
    );
    assert!(
        !settled(Format::Tiff, Colour::Cmyk, true).transparent,
        "CMYK's paper is the sheet"
    );
    assert!(!settled(Format::Tiff, Colour::Grey, true).transparent);
    assert!(settled(Format::Tiff, Colour::Rgb, true).transparent);
    assert!(settled(Format::Png, Colour::Grey, true).transparent);
    assert_eq!(
        settled(Format::Png, Colour::Cmyk, false).colour,
        Colour::Rgb,
        "PNG has no CMYK"
    );
    assert_eq!(
        settled(Format::WebP, Colour::Cmyk, false).colour,
        Colour::Rgb
    );
    assert_eq!(
        settled(Format::Jpeg, Colour::Cmyk, false).colour,
        Colour::Cmyk
    );

    // And a clear PNG in grey carries its alpha.
    let bytes = made(
        ImageOptions {
            transparent: true,
            ..at_72(Format::Png, Colour::Grey)
        },
        None,
    );
    let picture = image::load_from_memory(&bytes).expect("reads");
    assert!(matches!(picture.color(), image::ColorType::La8));
    assert_eq!(picture.to_luma_alpha8().get_pixel(150, 75).0[1], 0);
}

#[test]
fn a_clear_tiff_marks_its_fourth_sample_as_alpha() {
    let bytes = made(
        ImageOptions {
            transparent: true,
            ..at_72(Format::Tiff, Colour::Rgb)
        },
        None,
    );
    assert_eq!(
        tiff_tag(&bytes, tiff::tags::Tag::ExtraSamples).and_then(|v| v.into_u16().ok()),
        Some(2),
        "unassociated alpha"
    );
    let pixels = image::load_from_memory(&bytes).expect("reads").to_rgba8();
    assert_eq!(pixels.get_pixel(150, 75).0[3], 0);
    assert_eq!(pixels.get_pixel(50, 25).0, [0, 0, 0, 255]);
}
