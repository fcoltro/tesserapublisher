//! Pages as pictures: PNG, JPEG, TIFF and WebP.
//!
//! **Drawn from the PDF, not from the screen.** A page is written as a plain
//! PDF by the writer in this crate — the same geometry, the same shaped type,
//! the same placed artwork a printed proof gets — and that file is rendered
//! (`hayro`) at the resolution asked for. The picture is then what the page
//! prints as, and not what the canvas happens to show: no frame edges, no
//! guides, no empty picture box crosses, and nothing the screen draws
//! differently because it is a screen. The crate's rule stands, that an export
//! is never made from the screen's scene.
//!
//! The PDF is written with no press named, so the page is rendered in the
//! colours the screen shows. A picture asked for in grey is those colours'
//! lightness; one asked for in CMYK is converted through the press's own
//! profile, as a placed photograph is in a CMYK PDF, and carries that
//! profile so the program opening it knows which press the numbers are for.

use std::sync::Arc;

use tessera_color::managed::{Conversion, OutputProfile};
use tessera_document::ids::FrameId;
use tessera_document::intent::OutputIntent;
use tessera_geometry::{DocPoint, DocRect};
use tessera_layout::ResolvedPage;
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};

use crate::{ExportOptions, PdfError, Standard};

/// Which kind of picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Format {
    /// Lossless, and able to leave the paper clear.
    #[default]
    Png,
    /// Smaller, lossy, always on white; RGB, grey or CMYK.
    Jpeg,
    /// Lossless, for print: RGB, grey or CMYK, compressed without loss.
    Tiff,
    /// Lossless, for the web, and able to leave the paper clear.
    WebP,
}

/// The colours a picture is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Colour {
    #[default]
    Rgb,
    /// One channel: the lightness of each colour.
    Grey,
    /// The press's four inks, converted through its profile.
    Cmyk,
}

impl Colour {
    pub fn label(self) -> &'static str {
        match self {
            Colour::Rgb => "RGB",
            Colour::Grey => "Grey",
            Colour::Cmyk => "CMYK",
        }
    }
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Png, Format::Jpeg, Format::Tiff, Format::WebP];

    /// The extension a file of this kind takes.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Tiff => "tif",
            Format::WebP => "webp",
        }
    }

    /// Every extension a file of this kind is known by, the usual one first.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Format::Png => &["png"],
            Format::Jpeg => &["jpg", "jpeg"],
            Format::Tiff => &["tif", "tiff"],
            Format::WebP => &["webp"],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
            Format::Tiff => "TIFF",
            Format::WebP => "WebP",
        }
    }

    /// The colours a file of this kind can hold. CMYK only where a press
    /// reads it: JPEG and TIFF.
    pub fn colours(self) -> &'static [Colour] {
        match self {
            Format::Jpeg | Format::Tiff => &[Colour::Rgb, Colour::Grey, Colour::Cmyk],
            Format::Png | Format::WebP => &[Colour::Rgb, Colour::Grey],
        }
    }

    /// Whether it can leave the paper clear. A JPEG has no transparency.
    pub fn can_be_clear(self) -> bool {
        !matches!(self, Format::Jpeg)
    }

    /// Whether it loses detail to be smaller, which is what a quality is for.
    pub fn is_lossy(self) -> bool {
        matches!(self, Format::Jpeg)
    }
}

/// What pictures an export makes.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ImageOptions {
    pub format: Format,
    /// Pixels to the inch the page is drawn at: 72 is a point to a pixel.
    pub ppi: f64,
    pub colour: Colour,
    /// Leave the paper clear rather than white, where the format and the
    /// colours can: not in a JPEG, not in CMYK, whose paper is the sheet's,
    /// and not in a grey TIFF, which has no way to say so.
    pub transparent: bool,
    /// JPEG quality, 1 to 100.
    pub quality: u8,
    /// A JPEG that arrives coarse and sharpens as it loads, as a web page
    /// likes it, rather than top to bottom.
    pub progressive: bool,
    /// Carry the colour profile the numbers mean: sRGB for RGB, the press's
    /// for CMYK. Grey carries none.
    pub embed_profile: bool,
    /// Take in the bleed, where the page has one, rather than stop at the trim.
    pub bleed: bool,
}

impl Default for ImageOptions {
    fn default() -> Self {
        ImageOptions {
            format: Format::Png,
            ppi: 150.0,
            colour: Colour::Rgb,
            transparent: false,
            quality: QUALITY_LEVELS[2].1,
            progressive: false,
            embed_profile: true,
            bleed: false,
        }
    }
}

impl ImageOptions {
    /// These choices made consistent: a colour the format cannot hold is
    /// RGB, and clear paper the file cannot hold is white.
    pub fn settled(self) -> Self {
        let colour = if self.format.colours().contains(&self.colour) {
            self.colour
        } else {
            Colour::Rgb
        };
        let can_be_clear = self.format.can_be_clear()
            && colour != Colour::Cmyk
            && !(self.format == Format::Tiff && colour == Colour::Grey);
        ImageOptions {
            colour,
            transparent: self.transparent && can_be_clear,
            quality: self.quality.clamp(1, 100),
            ppi: self.ppi.clamp(*PPI_RANGE.start(), *PPI_RANGE.end()),
            ..self
        }
    }
}

/// InDesign's four JPEG qualities, and the number each is.
pub const QUALITY_LEVELS: [(&str, u8); 4] =
    [("Low", 30), ("Medium", 60), ("High", 80), ("Maximum", 100)];

/// The resolutions people ask for by name: a pixel to a point, a screen at
/// twice that, and print.
pub const USUAL_PPI: [f64; 4] = [72.0, 144.0, 150.0, 300.0];

/// The lowest and highest resolution a page is drawn at.
pub const PPI_RANGE: std::ops::RangeInclusive<f64> = 10.0..=2400.0;

/// The most pixels a side of a picture can have: the renderer's pixmap
/// counts its sides in sixteen bits.
pub const MOST_PIXELS_A_SIDE: u32 = u16::MAX as u32;

/// One page as a picture, encoded and ready to write.
#[derive(Debug, Clone)]
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

/// The area of `page` a picture of it shows: the trim, or the trim and its
/// bleed.
pub fn area(page: &ResolvedPage, options: &ImageOptions) -> DocRect {
    if options.bleed {
        page.bleed
    } else {
        page.bounds
    }
}

/// How many pixels a picture of `area` is at `ppi`: rounded, as a person
/// working it out on paper would, and never less than one.
pub fn pixels(area: DocRect, ppi: f64) -> (u64, u64) {
    let side = |points: f64| ((points * ppi / 72.0).round() as u64).max(1);
    (side(area.width), side(area.height))
}

/// The press a CMYK picture is converted for, and the profile it carries.
struct Press {
    conversion: Conversion,
    profile: Vec<u8>,
}

impl Press {
    fn of(intent: Option<&OutputIntent>) -> Result<Self, PdfError> {
        let no_press = || {
            PdfError::CannotConform(vec![
                "A CMYK picture needs a CMYK press profile, and none is named".to_string(),
            ])
        };
        let intent = intent.ok_or_else(no_press)?;
        let profile = OutputProfile::from_bytes(intent.profile.clone()).map_err(|_| no_press())?;
        if profile.space() != "CMYK" {
            return Err(no_press());
        }
        let conversion = profile
            .ink_for_screen_colour(intent.rendering.to_managed())
            .map_err(|_| no_press())?;
        Ok(Press {
            conversion,
            profile: intent.profile.clone(),
        })
    }
}

/// Every page of `resolved`, in order, as a picture. `press` is the profile
/// a CMYK picture is converted through; the other colours need none.
pub fn page_images(
    resolved: &ResolvedDocument,
    options: &ImageOptions,
    press: Option<&OutputIntent>,
) -> Result<Vec<PageImage>, PdfError> {
    let options = options.settled();
    let ppi = options.ppi;
    let press = match options.colour {
        Colour::Cmyk => Some(Press::of(press)?),
        _ => None,
    };
    let mut document = resolved.clone();
    for page in &mut document.pages {
        let (width, height) = pixels(area(page, &options), ppi);
        if width > u64::from(MOST_PIXELS_A_SIDE) || height > u64::from(MOST_PIXELS_A_SIDE) {
            return Err(PdfError::TooLarge {
                width,
                height,
                most: MOST_PIXELS_A_SIDE,
            });
        }
        // Without the bleed the written page stops at the trim: its media
        // box is the bleed box, and that is what is rendered, and the
        // writer clips every object to it.
        if !options.bleed {
            page.bleed = page.bounds;
        }
    }

    // Plain, unmarked, and for no press: a picture of the page, in the
    // colours the screen shows.
    let written = crate::export_with(
        &document,
        &ExportOptions {
            standard: Standard::Plain,
            marks: crate::Marks {
                crop: false,
                bleed: false,
                registration: false,
                colour_bar: false,
                ..crate::Marks::default()
            },
            intent: None,
            // A picture has no outline pane and nothing to click.
            bookmarks: false,
            hyperlinks: false,
            ..ExportOptions::default()
        },
    )?;
    let pdf = hayro::hayro_syntax::Pdf::new(Arc::new(written))
        .map_err(|e| PdfError::Encode(format!("the written pages would not read back: {e:?}")))?;
    let cache = hayro::RenderCache::new();
    let interpreter = hayro::hayro_interpret::InterpreterSettings::default();
    let srgb = (options.embed_profile && options.colour == Colour::Rgb)
        .then(|| OutputProfile::screen().map(|p| p.bytes().to_vec()))
        .flatten();

    let mut out = Vec::with_capacity(document.pages.len());
    for (page, rendered) in document.pages.iter().zip(pdf.pages().iter()) {
        let shown = area(page, &options);
        let (width, height) = pixels(shown, ppi);
        let (width, height) = (width as u16, height as u16);
        let settings = hayro::RenderSettings {
            // A scale for each side, so the page fills the whole picture
            // exactly however the pixel counts were rounded.
            x_scale: f32::from(width) / shown.width as f32,
            y_scale: f32::from(height) / shown.height as f32,
            width: Some(width),
            height: Some(height),
            bg_color: if options.transparent {
                hayro::vello_cpu::color::palette::css::TRANSPARENT
            } else {
                hayro::vello_cpu::color::palette::css::WHITE
            },
        };
        let pixmap = hayro::render(rendered, &cache, &interpreter, &settings);
        let rgba: Vec<u8> = pixmap
            .take_unpremultiplied()
            .into_iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect();
        let picture = Pixels {
            width: u32::from(width),
            height: u32::from(height),
            layout: match (options.colour, options.transparent) {
                (Colour::Rgb, false) => Layout::Rgb,
                (Colour::Rgb, true) => Layout::Rgba,
                (Colour::Grey, false) => Layout::Grey,
                (Colour::Grey, true) => Layout::GreyAlpha,
                (Colour::Cmyk, _) => Layout::Cmyk,
            },
            data: match (options.colour, options.transparent) {
                (Colour::Rgb, false) => rgb(&rgba),
                (Colour::Rgb, true) => rgba,
                (Colour::Grey, clear) => grey(&rgba, clear),
                (Colour::Cmyk, _) => inks(
                    &rgb(&rgba),
                    &press.as_ref().expect("a press for CMYK").conversion,
                ),
            },
        };
        let profile = match options.colour {
            Colour::Rgb => srgb.as_deref(),
            Colour::Cmyk if options.embed_profile => press.as_ref().map(|p| p.profile.as_slice()),
            _ => None,
        };
        let bytes = match options.format {
            Format::Png => png(&picture, ppi, profile.is_some())?,
            Format::Jpeg => jpeg(&picture, ppi, &options, profile)?,
            Format::Tiff => tiff(&picture, ppi, profile)?,
            Format::WebP => webp(&picture, profile)?,
        };
        out.push(PageImage {
            width: picture.width,
            height: picture.height,
            bytes,
        });
    }
    Ok(out)
}

/// How a picture's samples are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Rgb,
    Rgba,
    Grey,
    GreyAlpha,
    /// Ink amounts, 255 for full ink.
    Cmyk,
}

struct Pixels {
    width: u32,
    height: u32,
    layout: Layout,
    data: Vec<u8>,
}

/// `pixels` as a PNG that says its resolution, so a program placing it knows
/// the size it was made for, and — for RGB with a profile asked for — that
/// its numbers are sRGB, which PNG says in a chunk of its own rather than a
/// whole profile.
fn png(pixels: &Pixels, ppi: f64, srgb: bool) -> Result<Vec<u8>, PdfError> {
    let failed = |e: png::EncodingError| PdfError::Encode(e.to_string());
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, pixels.width, pixels.height);
    encoder.set_color(match pixels.layout {
        Layout::Rgb => png::ColorType::Rgb,
        Layout::Rgba => png::ColorType::Rgba,
        Layout::Grey => png::ColorType::Grayscale,
        Layout::GreyAlpha => png::ColorType::GrayscaleAlpha,
        Layout::Cmyk => return Err(PdfError::Encode("PNG has no CMYK".into())),
    });
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);
    let per_metre = (ppi / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: per_metre,
        yppu: per_metre,
        unit: png::Unit::Meter,
    }));
    if srgb {
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    }
    let mut writer = encoder.write_header().map_err(failed)?;
    writer.write_image_data(&pixels.data).map_err(failed)?;
    writer.finish().map_err(failed)?;
    Ok(out)
}

/// `pixels` — on white, since a JPEG cannot be anything else — as a JPEG
/// that says its resolution and, when asked, carries its profile.
fn jpeg(
    pixels: &Pixels,
    ppi: f64,
    options: &ImageOptions,
    profile: Option<&[u8]>,
) -> Result<Vec<u8>, PdfError> {
    use jpeg_encoder::{ColorType, Encoder, PixelDensity};

    let failed = |e: jpeg_encoder::EncodingError| PdfError::Encode(e.to_string());
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, options.quality);
    encoder.set_density(PixelDensity::dpi(ppi.round().clamp(1.0, 65535.0) as u16));
    encoder.set_progressive(options.progressive);
    if let Some(profile) = profile {
        encoder.add_icc_profile(profile).map_err(failed)?;
    }
    let (data, colour) = match pixels.layout {
        Layout::Rgb => (pixels.data.clone(), ColorType::Rgb),
        Layout::Grey => (pixels.data.clone(), ColorType::Luma),
        // Adobe's own way, which is what a press's RIP expects: the file
        // says it holds CMYK and the reader takes it as such.
        Layout::Cmyk => (pixels.data.clone(), ColorType::Cmyk),
        Layout::Rgba | Layout::GreyAlpha => {
            return Err(PdfError::Encode("a JPEG cannot be clear".into()));
        }
    };
    encoder
        .encode(&data, pixels.width as u16, pixels.height as u16, colour)
        .map_err(failed)?;
    Ok(out)
}

/// `pixels` as a TIFF, compressed without loss (LZW, with the horizontal
/// predictor that makes it pay on continuous tone), saying its resolution
/// and carrying its profile when asked.
fn tiff(pixels: &Pixels, ppi: f64, profile: Option<&[u8]>) -> Result<Vec<u8>, PdfError> {
    use tiff::encoder::{Compression, Predictor, Rational, TiffEncoder, colortype};
    use tiff::tags::{ExtraSamples, ResolutionUnit, Tag};

    let failed = |e: tiff::TiffError| PdfError::Encode(e.to_string());
    let mut out = std::io::Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut out)
        .map_err(failed)?
        .with_compression(Compression::Lzw)
        .with_predictor(Predictor::Horizontal);
    let resolution = Rational {
        n: (ppi * 100.0).round() as u32,
        d: 100,
    };
    macro_rules! write {
        ($kind:ty, $alpha:expr) => {{
            let mut image = encoder
                .new_image::<$kind>(pixels.width, pixels.height)
                .map_err(failed)?;
            if $alpha {
                // Written as a tag rather than through `extra_samples`,
                // which counts the alpha on top of the colour type's own
                // samples while the predictor still steps by the colour
                // type's count, and scrambles every row.
                image
                    .encoder()
                    .write_tag(
                        Tag::ExtraSamples,
                        &[ExtraSamples::UnassociatedAlpha.to_u16()][..],
                    )
                    .map_err(failed)?;
            }
            image.resolution(ResolutionUnit::Inch, resolution);
            if let Some(profile) = profile {
                image
                    .encoder()
                    .write_tag(Tag::IccProfile, profile)
                    .map_err(failed)?;
            }
            image.write_data(&pixels.data).map_err(failed)?;
        }};
    }
    match pixels.layout {
        Layout::Rgb => write!(colortype::RGB8, false),
        // Four samples, the fourth said to be the alpha.
        Layout::Rgba => write!(colortype::RGBA8, true),
        Layout::Grey => write!(colortype::Gray8, false),
        Layout::Cmyk => write!(colortype::CMYK8, false),
        Layout::GreyAlpha => {
            return Err(PdfError::Encode("a grey TIFF cannot be clear here".into()));
        }
    }
    Ok(out.into_inner())
}

/// `pixels` as a lossless WebP, carrying its profile when asked. WebP has
/// no place for a resolution, and no grey of its own: grey is written as
/// three equal channels, which its lossless coding stores for little more
/// than one.
fn webp(pixels: &Pixels, profile: Option<&[u8]>) -> Result<Vec<u8>, PdfError> {
    use image::ImageEncoder;

    let failed = |e: image::ImageError| PdfError::Encode(e.to_string());
    let mut out = Vec::new();
    let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut out);
    if let Some(profile) = profile {
        encoder
            .set_icc_profile(profile.to_vec())
            .map_err(|e| PdfError::Encode(e.to_string()))?;
    }
    let colour = match pixels.layout {
        Layout::Rgb => image::ExtendedColorType::Rgb8,
        Layout::Rgba => image::ExtendedColorType::Rgba8,
        Layout::Grey => image::ExtendedColorType::L8,
        Layout::GreyAlpha => image::ExtendedColorType::La8,
        Layout::Cmyk => return Err(PdfError::Encode("WebP has no CMYK".into())),
    };
    encoder
        .write_image(&pixels.data, pixels.width, pixels.height, colour)
        .map_err(failed)?;
    Ok(out)
}

fn rgb(rgba: &[u8]) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks::<4>();
    pixels.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
}

/// Each pixel's lightness, weighted as the eye weighs red, green and blue
/// (Rec. 709), and its alpha after it when the paper is clear.
fn grey(rgba: &[u8], clear: bool) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks::<4>();
    let mut out = Vec::with_capacity(pixels.len() * if clear { 2 } else { 1 });
    for [r, g, b, a] in pixels {
        let y = 0.2126 * f32::from(*r) + 0.7152 * f32::from(*g) + 0.0722 * f32::from(*b);
        out.push(y.round().clamp(0.0, 255.0) as u8);
        if clear {
            out.push(*a);
        }
    }
    out
}

/// How many pixels are converted per call into Little CMS, as for a placed
/// photograph: big enough that the call costs nothing, small enough that the
/// scratch space does not.
const CHUNK: usize = 1 << 16;

/// RGB pixels as the press's inks, 255 for full ink.
///
/// **Pure black is one ink**, as it is for colours in a CMYK PDF: black type
/// drawn on white is black where it is solid, and through the press profile
/// that black would print in all four inks and fringe wherever the plates
/// slip.
fn inks(rgb: &[u8], conversion: &Conversion) -> Vec<u8> {
    let (pixels, _) = rgb.as_chunks::<3>();
    let mut out = Vec::with_capacity(pixels.len() * 4);
    for block in pixels.chunks(CHUNK) {
        let source: Vec<[f32; 3]> = block
            .iter()
            .map(|p| p.map(|v| f32::from(v) / 255.0))
            .collect();
        for (pixel, ink) in block.iter().zip(conversion.apply_run(&source)) {
            if *pixel == [0, 0, 0] {
                out.extend_from_slice(&[0, 0, 0, 255]);
            } else {
                out.extend(ink.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
        }
    }
    out
}

/// `frames` alone, on a page of their own just big enough to hold what they
/// paint — strokes and shadows included — so a selection exports as a
/// picture of itself. `None` when none of them paints anything.
///
/// A frame on a parent page is resolved once for every page showing it; the
/// first of those is the one taken, so the cut-out is one copy of it and not
/// a page-spanning strip of every copy.
pub fn cut_out(resolved: &ResolvedDocument, frames: &[FrameId]) -> Option<ResolvedDocument> {
    let mut taken: Vec<(FrameId, Option<tessera_document::ids::PageId>)> = Vec::new();
    let mut items = Vec::new();
    for item in &resolved.items {
        if !frames.contains(&item.frame) {
            continue;
        }
        match taken.iter().find(|(frame, _)| *frame == item.frame) {
            Some((_, on)) if *on != item.on => continue,
            Some(_) => {}
            None => taken.push((item.frame, item.on)),
        }
        items.push(item.clone());
    }
    let area = items
        .iter()
        .map(painted)
        .reduce(|a, b| {
            let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
            let (x1, y1) = (
                (a.x + a.width).max(b.x + b.width),
                (a.y + a.height).max(b.y + b.height),
            );
            DocRect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            }
        })
        .filter(|a| a.width > 0.0 && a.height > 0.0)?;
    Some(ResolvedDocument {
        items,
        pages: vec![ResolvedPage {
            bounds: area,
            margins: area,
            bleed: area,
            slug: area,
            columns: Vec::new(),
        }],
        bookmarks: Vec::new(),
    })
}

/// The box an item paints into: its frame turned as it is turned, out to
/// where its stroke reaches, and out to where its shadow falls.
fn painted(item: &ResolvedItem) -> DocRect {
    let b = item.bounds;
    let reach = stroke_of(&item.kind).map_or(0.0, |s| (s.width / 2.0 + s.offset()).max(0.0));
    let corners = [
        DocPoint {
            x: b.x - reach,
            y: b.y - reach,
        },
        DocPoint {
            x: b.x + b.width + reach,
            y: b.y - reach,
        },
        DocPoint {
            x: b.x - reach,
            y: b.y + b.height + reach,
        },
        DocPoint {
            x: b.x + b.width + reach,
            y: b.y + b.height + reach,
        },
    ]
    .map(|p| item.transform.apply(p));
    let (mut x0, mut y0, mut x1, mut y1) = corners.iter().fold(
        (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
        |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
    );
    if let Some(shadow) = item.shadow.as_ref().filter(|s| !s.is_invisible()) {
        let spread = 2.5 * shadow.std_dev();
        let (dx, dy) = shadow.offset;
        x0 = x0.min(x0 + dx - spread);
        y0 = y0.min(y0 + dy - spread);
        x1 = x1.max(x1 + dx + spread);
        y1 = y1.max(y1 + dy + spread);
    }
    DocRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

fn stroke_of(kind: &ResolvedKind) -> Option<&tessera_document::nodes::Stroke> {
    match kind {
        ResolvedKind::Rectangle { stroke, .. }
        | ResolvedKind::Ellipse { stroke, .. }
        | ResolvedKind::Table { stroke, .. }
        | ResolvedKind::Path { stroke, .. }
        | ResolvedKind::Graphic { stroke, .. } => stroke.as_ref(),
        _ => None,
    }
}
