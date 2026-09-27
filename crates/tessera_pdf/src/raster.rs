//! Pages as pictures: PNG and JPEG.
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
//! The PDF is written with no press named, so colour is what the screen shows
//! too: RGB as it is, CMYK and spot colours as their screen colour. A picture
//! is for a screen or a web page, not for a press.

use std::sync::Arc;

use tessera_document::ids::FrameId;
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
    /// Smaller, lossy, always on white.
    Jpeg,
}

impl Format {
    /// The extension a file of this kind takes.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
        }
    }
}

/// What pictures an export makes.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageOptions {
    pub format: Format,
    /// Pixels to the inch the page is drawn at: 72 is a point to a pixel.
    pub ppi: f64,
    /// Leave the paper clear rather than white. PNG only: a JPEG has no
    /// transparency, and is always on white.
    pub transparent: bool,
    /// JPEG quality, 1 to 100.
    pub quality: u8,
    /// Take in the bleed, where the page has one, rather than stop at the trim.
    pub bleed: bool,
}

impl Default for ImageOptions {
    fn default() -> Self {
        ImageOptions {
            format: Format::Png,
            ppi: 150.0,
            transparent: false,
            quality: 90,
            bleed: false,
        }
    }
}

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

/// Every page of `resolved`, in order, as a picture.
pub fn page_images(
    resolved: &ResolvedDocument,
    options: &ImageOptions,
) -> Result<Vec<PageImage>, PdfError> {
    let ppi = options.ppi.clamp(*PPI_RANGE.start(), *PPI_RANGE.end());
    let mut document = resolved.clone();
    for page in &mut document.pages {
        let (width, height) = pixels(area(page, options), ppi);
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
        },
    )?;
    let pdf = hayro::hayro_syntax::Pdf::new(Arc::new(written))
        .map_err(|e| PdfError::Encode(format!("the written pages would not read back: {e:?}")))?;
    let cache = hayro::RenderCache::new();
    let interpreter = hayro::hayro_interpret::InterpreterSettings::default();

    let mut out = Vec::with_capacity(document.pages.len());
    for (page, rendered) in document.pages.iter().zip(pdf.pages().iter()) {
        let shown = area(page, options);
        let (width, height) = pixels(shown, ppi);
        let (width, height) = (width as u16, height as u16);
        let clear = options.transparent && options.format == Format::Png;
        let settings = hayro::RenderSettings {
            // A scale for each side, so the page fills the whole picture
            // exactly however the pixel counts were rounded.
            x_scale: f32::from(width) / shown.width as f32,
            y_scale: f32::from(height) / shown.height as f32,
            width: Some(width),
            height: Some(height),
            bg_color: if clear {
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
        let (width, height) = (u32::from(width), u32::from(height));
        let bytes = match options.format {
            Format::Png => png(&rgba, width, height, ppi, clear)?,
            Format::Jpeg => jpeg(&rgba, width, height, ppi, options.quality)?,
        };
        out.push(PageImage {
            width,
            height,
            bytes,
        });
    }
    Ok(out)
}

/// `rgba` as a PNG that says its resolution, so a program placing it knows
/// the size it was made for. Without the alpha channel when the paper is
/// white: a channel that is 255 everywhere is a quarter of the file for
/// nothing.
fn png(rgba: &[u8], width: u32, height: u32, ppi: f64, clear: bool) -> Result<Vec<u8>, PdfError> {
    let failed = |e: png::EncodingError| PdfError::Encode(e.to_string());
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(if clear {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    });
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);
    let per_metre = (ppi / 0.0254).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: per_metre,
        yppu: per_metre,
        unit: png::Unit::Meter,
    }));
    let mut writer = encoder.write_header().map_err(failed)?;
    if clear {
        writer.write_image_data(rgba).map_err(failed)?;
    } else {
        writer.write_image_data(&rgb(rgba)).map_err(failed)?;
    }
    writer.finish().map_err(failed)?;
    Ok(out)
}

/// `rgba` — opaque, since it was drawn on white — as a JPEG that says its
/// resolution.
fn jpeg(rgba: &[u8], width: u32, height: u32, ppi: f64, quality: u8) -> Result<Vec<u8>, PdfError> {
    use image::codecs::jpeg::{JpegEncoder, PixelDensity};

    let mut out = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    encoder.set_pixel_density(PixelDensity::dpi(ppi.round().clamp(1.0, 65535.0) as u16));
    encoder
        .encode(&rgb(rgba), width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| PdfError::Encode(e.to_string()))?;
    Ok(out)
}

fn rgb(rgba: &[u8]) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks::<4>();
    pixels.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
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
