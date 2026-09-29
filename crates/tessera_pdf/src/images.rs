//! Placed artwork, as bytes a PDF can carry.
//!
//! Until this, `ResolvedKind::Graphic` was skipped by the writer and a page of
//! photographs exported as a page of nothing. The placeholder was never written
//! — a violet cross in a printed job is far worse than a blank space — so a job
//! with pictures came out looking finished and empty.
//!
//! ## A JPEG is passed through, not decoded
//!
//! PDF's `/DCTDecode` filter *is* JPEG. Handing the file's own bytes to the
//! reader is smaller than anything this could re-encode, and exactly as good as
//! the original — decoding and re-encoding would lose a generation of quality
//! for no reason and take longer doing it. Everything else is decoded and
//! deflated.
//!
//! ## Transparency is a separate greyscale image
//!
//! PDF has no RGBA. An alpha channel becomes an `/SMask`: a second image, one
//! component per pixel, named by the first. Dropping the alpha instead would
//! composite a cut-out photograph onto black.

use std::path::Path;

use crate::PdfError as Error;
use tessera_color::managed::Conversion;
use tessera_document::links::PdfPage;

/// How the bytes are compressed, which is the same thing as which PDF filter
/// reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coding {
    /// The file's own JPEG bytes, untouched.
    Jpeg,
    /// Raw samples, deflated.
    Flate,
}

/// How many components a prepared image's samples carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    Rgb,
    /// Converted through the press's own profile.
    Cmyk,
}

/// One image, ready to be written.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub width: u32,
    pub height: u32,
    pub coding: Coding,
    pub space: Space,
    /// The colour samples: JPEG bytes, or deflated RGB triples.
    pub data: Vec<u8>,
    /// The alpha channel, deflated, one byte per pixel. `None` when the artwork
    /// is opaque — and it is left `None` rather than filled with 255s, because a
    /// fully opaque soft mask is a second image the size of the first that
    /// changes nothing.
    pub alpha: Option<Vec<u8>>,
}

impl Prepared {
    /// Whether this needs an `/SMask`.
    pub fn is_transparent(&self) -> bool {
        self.alpha.is_some()
    }
}

/// Read a placed file and prepare it for embedding.
///
/// **Failure here is not fatal to the export.** A link that has gone missing or
/// turned out to be something this cannot read is a preflight problem, already
/// reported; refusing to write the whole PDF because of one picture would mean a
/// job with a broken link cannot be proofed at all.
#[cfg(test)]
pub fn prepare(path: &Path) -> Result<Prepared, Error> {
    prepare_page(path, PdfPage::default())
}

/// [`prepare`] for a placed PDF's chosen page and box.
fn prepare_page(path: &Path, pdf: PdfPage) -> Result<Prepared, Error> {
    if tessera_render::images::is_vector(path) {
        return prepare_vector(path, pdf);
    }
    if tessera_render::eps::is_eps(path) {
        let (rgba, (w, h)) = eps_preview(path)?;
        return Ok(from_rgba(&rgba, w, h));
    }
    let bytes = std::fs::read(path)?;
    if tessera_render::psd::is_psd(path) {
        let composite = photoshop(path, &bytes)?;
        return Ok(from_rgba(
            &composite.rgba,
            composite.width,
            composite.height,
        ));
    }

    if is_jpeg(&bytes) {
        // Dimensions from the file's own header rather than by decoding it:
        // reading a marker chain is a few hundred bytes of work where decoding
        // a 40-megapixel photograph is a second and a hundred megabytes.
        if let Some((width, height, 8, 3)) = jpeg_header(&bytes) {
            return Ok(Prepared {
                width,
                height,
                coding: Coding::Jpeg,
                space: Space::Rgb,
                data: bytes,
                // A baseline JPEG has no alpha. There is nothing to look for.
                alpha: None,
            });
        }
        // Grayscale, CMYK and non-8-bit JPEGs cannot wear an RGB/8-bit label.
        // Decode them to actual RGB samples, as well as unrecognized headers.
    }

    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| Error::Unreadable(path.to_path_buf(), e.to_string()))?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(from_rgba(rgba.as_raw(), width, height))
}

/// A Photoshop file's flattened picture.
fn photoshop(path: &Path, bytes: &[u8]) -> Result<tessera_render::psd::Composite, Error> {
    tessera_render::psd::read(bytes).map_err(|why| {
        Error::Unreadable(
            path.to_path_buf(),
            format!("not readable as Photoshop: {why:?}"),
        )
    })
}

/// Straight RGBA as deflated RGB, and its alpha as a mask when any of it is
/// less than opaque.
fn from_rgba(rgba: &[u8], width: u32, height: u32) -> Prepared {
    let mut colour = Vec::with_capacity((width * height * 3) as usize);
    let mut alpha = Vec::with_capacity((width * height) as usize);
    let mut any_transparent = false;
    // `as_chunks` rather than `chunks_exact(4)`: the width is a constant, so
    // this hands back `[u8; 4]` and the indexing below is checked once at
    // compile time instead of four times a pixel.
    let (pixels, _) = rgba.as_chunks::<4>();
    for pixel in pixels {
        colour.extend_from_slice(&pixel[..3]);
        alpha.push(pixel[3]);
        any_transparent |= pixel[3] != 255;
    }

    Prepared {
        width,
        height,
        coding: Coding::Flate,
        space: Space::Rgb,
        data: deflate(&colour),
        alpha: any_transparent.then(|| deflate(&alpha)),
    }
}

/// How finely placed vector artwork is rendered, when it has to be.
///
/// **Rasterised, and this is the compromise to know about.** Vector artwork
/// ought to reach a PDF as vectors; the crate that does that conversion for an
/// SVG (`svg2pdf`) is built against `pdf-writer` 0.12 and this writes with
/// 0.15, so their types cannot meet. Until they line up, an SVG is rendered at
/// a resolution high enough that a press will not show it — 600 pixels per
/// inch is twice what a 300ppi photograph gets and is the usual number for
/// line work — and the limitation is written down here rather than discovered
/// on a proof.
///
/// A placed PDF is copied across as vectors by the writer, and comes here only
/// for an export converted into a press's inks: its own colours are whatever
/// its maker chose, often RGB, and a CMYK export has no RGB left in it.
const VECTOR_PPI: f64 = 600.0;

/// A placed SVG or PDF rendered for the page, at [`VECTOR_PPI`].
///
/// One function because both the RGB path and the CMYK one need the same
/// pixels; rendering twice would be slow and could disagree.
fn vector_rgba(path: &Path, page: PdfPage) -> Result<(Vec<u8>, u32, u32), Error> {
    let pdf = tessera_render::images::is_pdf(path);
    let kind = if pdf { "PDF" } else { "SVG" };
    let unreadable =
        |why: &str| Error::Unreadable(path.to_path_buf(), format!("not readable as {kind}: {why}"));

    let size = if pdf {
        tessera_render::images::pdf_page_size(path, page)
    } else {
        tessera_render::images::svg_size(path)
    };
    let (natural_w, natural_h) = size.ok_or_else(|| unreadable("it does not parse"))?;
    if !(natural_w > 0.0 && natural_h > 0.0) {
        return Err(unreadable("the drawing has no size"));
    }

    // Points to pixels at the chosen resolution, asked of the same renderer the
    // screen uses so the proof and the page cannot disagree about the artwork.
    let longest = natural_w.max(natural_h);
    let edge = (longest / 72.0 * VECTOR_PPI)
        .round()
        .clamp(1.0, f64::from(u32::MAX)) as u32;

    let rendered = if pdf {
        tessera_render::images::render_pdf_page(path, page, edge)
    } else {
        tessera_render::images::render_svg(path, edge)
    };
    let (rgba, (width, height)) = rendered.ok_or_else(|| unreadable("it renders to nothing"))?;
    Ok((rgba, width, height))
}

/// Render placed vector artwork into pixels for embedding.
fn prepare_vector(path: &Path, pdf: PdfPage) -> Result<Prepared, Error> {
    let (rgba, width, height) = vector_rgba(path, pdf)?;
    Ok(from_rgba(&rgba, width, height))
}

/// How many pixels are converted per call into Little CMS.
///
/// A chunk rather than the whole image, because the buffers are `[f32; 3]` and
/// `[f32; 4]` going in and out: a forty-megapixel scan converted in one call
/// wants a gigabyte of scratch for a picture that is a fifth of that on disk.
/// Big enough that the per-call cost disappears, small enough to be free.
const CHUNK: usize = 1 << 16;

/// The same artwork, converted into the press's inks.
///
/// **A JPEG cannot be passed through here.** `/DCTDecode` carries the file's own
/// bytes, and those bytes are RGB — converting means decoding, so the
/// pass-through that makes an RGB export cheap is exactly what a CMYK export
/// cannot have. That is a real cost of a CMYK export and not a shortcut worth
/// looking for: the alternative is a file whose pictures are in the wrong space.
///
/// Alpha survives. It is coverage, not colour, and has nothing to do with which
/// inks the picture is made of.
#[cfg(test)]
pub fn to_cmyk(path: &Path, conversion: &Conversion) -> Result<Prepared, Error> {
    to_cmyk_page(path, PdfPage::default(), conversion)
}

/// [`to_cmyk`] for a placed PDF's chosen page and box.
fn to_cmyk_page(path: &Path, pdf: PdfPage, conversion: &Conversion) -> Result<Prepared, Error> {
    // A drawing is rendered rather than decoded, and then converted like any
    // other picture: the inks a drawing prints in are the press's business, not
    // the drawing's.
    let malformed = || Error::Unreadable(path.to_path_buf(), "malformed rendering".into());
    let rgba = if tessera_render::images::is_vector(path) {
        let (raw, w, h) = vector_rgba(path, pdf)?;
        image::RgbaImage::from_raw(w, h, raw).ok_or_else(malformed)?
    } else if tessera_render::eps::is_eps(path) {
        let (raw, (w, h)) = eps_preview(path)?;
        image::RgbaImage::from_raw(w, h, raw).ok_or_else(malformed)?
    } else if tessera_render::psd::is_psd(path) {
        let composite = photoshop(path, &std::fs::read(path)?)?;
        if let Some(inks) = composite.inks {
            // Already in inks, and the retoucher's own numbers: converted
            // through RGB and back they would all move, and the black a
            // retoucher kept to one plate would come back in all four.
            let alpha: Vec<u8> = composite.rgba.iter().skip(3).step_by(4).copied().collect();
            let any_transparent = alpha.iter().any(|&a| a != 255);
            return Ok(Prepared {
                width: composite.width,
                height: composite.height,
                coding: Coding::Flate,
                space: Space::Cmyk,
                data: deflate(&inks),
                alpha: any_transparent.then(|| deflate(&alpha)),
            });
        }
        image::RgbaImage::from_raw(composite.width, composite.height, composite.rgba)
            .ok_or_else(malformed)?
    } else {
        let bytes = std::fs::read(path)?;
        image::load_from_memory(&bytes)
            .map_err(|e| Error::Unreadable(path.to_path_buf(), e.to_string()))?
            .to_rgba8()
    };
    Ok(cmyk_from_rgba(&rgba, conversion))
}

/// Straight RGBA converted through `conversion` into inks, deflated, with
/// its alpha as a mask when any of it is less than opaque.
fn cmyk_from_rgba(rgba: &image::RgbaImage, conversion: &Conversion) -> Prepared {
    let (width, height) = rgba.dimensions();

    let mut inks: Vec<u8> = Vec::with_capacity((width * height * 4) as usize);
    let mut alpha: Vec<u8> = Vec::with_capacity((width * height) as usize);
    let mut any_transparent = false;

    let pixels: Vec<_> = rgba.pixels().collect();
    for block in pixels.chunks(CHUNK) {
        let source: Vec<[f32; 3]> = block
            .iter()
            .map(|p| {
                [
                    f32::from(p.0[0]) / 255.0,
                    f32::from(p.0[1]) / 255.0,
                    f32::from(p.0[2]) / 255.0,
                ]
            })
            .collect();

        for ink in conversion.apply_run(&source) {
            for channel in ink {
                inks.push((channel.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
        for p in block {
            alpha.push(p.0[3]);
            any_transparent |= p.0[3] != 255;
        }
    }

    Prepared {
        width,
        height,
        coding: Coding::Flate,
        space: Space::Cmyk,
        data: deflate(&inks),
        alpha: any_transparent.then(|| deflate(&alpha)),
    }
}

/// How finely a picture drawn at `drawn` points is sampled, in pixels an
/// inch: the coarser of its two directions, which is the one a reader sees.
pub(crate) fn lowest_ppi(pixels: (u32, u32), drawn: (f64, f64)) -> f64 {
    let along = |pixels: u32, points: f64| f64::from(pixels) * 72.0 / points.max(f64::EPSILON);
    along(pixels.0, drawn.0).min(along(pixels.1, drawn.1))
}

/// How many pixels a placed file is, without decoding it: a drawing at the
/// resolution it is rendered at, a photograph from its header.
fn pixel_size(path: &Path, pdf: PdfPage) -> Option<(u32, u32)> {
    let rendered = |(w, h): (f64, f64)| {
        let side = |points: f64| (points / 72.0 * VECTOR_PPI).round() as u32;
        (side(w), side(h))
    };
    if tessera_render::images::is_pdf(path) {
        tessera_render::images::pdf_page_size(path, pdf).map(rendered)
    } else if tessera_render::images::is_svg(path) {
        tessera_render::images::svg_size(path).map(rendered)
    } else if tessera_render::eps::is_eps(path) {
        tessera_render::eps::preview(path).map(|(_, size)| size)
    } else if tessera_render::psd::is_psd(path) {
        tessera_render::psd::size(path)
    } else {
        image::image_dimensions(path).ok()
    }
}

/// An EPS's preview, for an EPS Ghostscript has not made a PDF of.
fn eps_preview(path: &Path) -> Result<(Vec<u8>, (u32, u32)), Error> {
    tessera_render::eps::preview(path).ok_or_else(|| {
        Error::Unreadable(
            path.to_path_buf(),
            "an EPS with no preview, and no Ghostscript to draw it".into(),
        )
    })
}

/// A placed file as this export's pictures ask: brought down to the
/// resolution asked when it is drawn finer than asked — `drawn` is the
/// largest it is drawn anywhere, in points, so no use of it goes soft — and
/// compressed as asked.
///
/// **JPEG is for RGB only.** A CMYK JPEG in a PDF is stored inverted, as
/// Adobe writes it, and readers disagree about undoing that; a CMYK
/// picture is compressed without loss rather than risk printing its
/// negative.
///
/// What is left alone is left exactly as [`prepare`] and [`to_cmyk`] leave
/// it: a JPEG passed through byte for byte.
///
/// For a PDF, `pdf` is the page and the box it is cut to: the same file at
/// another page is other artwork.
pub fn prepare_page_for(
    path: &Path,
    pdf: PdfPage,
    conversion: Option<&Conversion>,
    pictures: &crate::Pictures,
    drawn: (f64, f64),
) -> Result<Prepared, Error> {
    use crate::Compression;

    // An EPS is its PDF when Ghostscript has made one.
    let converted = tessera_render::eps::effective(path);
    let path = converted.as_path();
    let plain = || match conversion {
        Some(conversion) => to_cmyk_page(path, pdf, conversion),
        None => prepare_page(path, pdf),
    };
    if pictures.downsample.is_none() && pictures.compression == Compression::Automatic {
        return plain();
    }
    let scale = match (pictures.downsample, pixel_size(path, pdf)) {
        (Some(down), Some(pixels)) if pixels.0 > 0 && pixels.1 > 0 => {
            let ppi = lowest_ppi(pixels, drawn);
            (ppi > down.above && down.to > 0.0).then(|| (down.to / ppi).min(1.0))
        }
        _ => None,
    };
    let is_jpeg_file =
        !tessera_render::images::is_vector(path) && !tessera_render::psd::is_psd(path) && {
            use std::io::Read;
            let mut start = [0u8; 2];
            std::fs::File::open(path)
                .and_then(|mut file| file.read_exact(&mut start))
                .is_ok_and(|()| is_jpeg(&start))
        };
    // JPEG out, for an RGB picture: when asked, or when the file was one.
    let jpeg_out = match pictures.compression {
        Compression::Jpeg => true,
        Compression::Automatic => is_jpeg_file,
        Compression::Zip => false,
    };
    let unchanged = scale.is_none()
        && match pictures.compression {
            Compression::Automatic => true,
            // Already JPEG. (A CMYK picture stays lossless whatever is asked.)
            Compression::Jpeg => is_jpeg_file,
            // Lossless already, unless a JPEG would be passed through.
            Compression::Zip => !(is_jpeg_file && conversion.is_none()),
        };
    if unchanged {
        return plain();
    }

    let unreadable = |e: String| Error::Unreadable(path.to_path_buf(), e);
    // The pixels, and a CMYK Photoshop file's own inks beside them.
    let (mut rgba, mut inks) = if tessera_render::images::is_vector(path) {
        let (raw, w, h) = vector_rgba(path, pdf)?;
        let rgba = image::RgbaImage::from_raw(w, h, raw)
            .ok_or_else(|| unreadable("malformed rendering".into()))?;
        (rgba, None)
    } else if tessera_render::psd::is_psd(path) {
        let composite = photoshop(path, &std::fs::read(path)?)?;
        let inks = composite
            .inks
            .filter(|_| conversion.is_some())
            .and_then(|inks| image::RgbaImage::from_raw(composite.width, composite.height, inks));
        let rgba = image::RgbaImage::from_raw(composite.width, composite.height, composite.rgba)
            .ok_or_else(|| unreadable("malformed composite".into()))?;
        (rgba, inks)
    } else if tessera_render::eps::is_eps(path) {
        let (raw, (w, h)) = eps_preview(path)?;
        let rgba = image::RgbaImage::from_raw(w, h, raw)
            .ok_or_else(|| unreadable("malformed preview".into()))?;
        (rgba, None)
    } else {
        let bytes = std::fs::read(path)?;
        let rgba = image::load_from_memory(&bytes)
            .map_err(|e| unreadable(e.to_string()))?
            .to_rgba8();
        (rgba, None)
    };
    if let Some(scale) = scale {
        // Bicubic, as InDesign's downsampling is: sharper than averaging and
        // without the ringing of anything sharper still.
        let (w, h) = rgba.dimensions();
        let (nw, nh) = (
            ((f64::from(w) * scale).round() as u32).max(1),
            ((f64::from(h) * scale).round() as u32).max(1),
        );
        let filter = image::imageops::FilterType::CatmullRom;
        rgba = image::imageops::resize(&rgba, nw, nh, filter);
        // Four inks resample as four channels, as RGBA's four do.
        inks = inks.map(|inks| image::imageops::resize(&inks, nw, nh, filter));
    }

    let (width, height) = rgba.dimensions();
    match conversion {
        Some(conversion) => Ok(match inks {
            Some(inks) => {
                let alpha: Vec<u8> = rgba.pixels().map(|p| p.0[3]).collect();
                let any_transparent = alpha.iter().any(|&a| a != 255);
                Prepared {
                    width,
                    height,
                    coding: Coding::Flate,
                    space: Space::Cmyk,
                    data: deflate(inks.as_raw()),
                    alpha: any_transparent.then(|| deflate(&alpha)),
                }
            }
            None => cmyk_from_rgba(&rgba, conversion),
        }),
        None if jpeg_out => {
            let mut ready = from_rgba(rgba.as_raw(), width, height);
            if let Some(jpeg) = jpeg_bytes(&rgba, pictures.quality) {
                ready.coding = Coding::Jpeg;
                ready.data = jpeg;
            }
            Ok(ready)
        }
        None => Ok(from_rgba(rgba.as_raw(), width, height)),
    }
}

/// The colour of `rgba` as a baseline JPEG, or `None` for one too big for
/// JPEG's sixteen-bit sides — left lossless rather than refused.
fn jpeg_bytes(rgba: &image::RgbaImage, quality: u8) -> Option<Vec<u8>> {
    let (w, h) = rgba.dimensions();
    let (w, h) = (u16::try_from(w).ok()?, u16::try_from(h).ok()?);
    let rgb: Vec<u8> = rgba
        .pixels()
        .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
        .collect();
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100))
        .encode(&rgb, w, h, jpeg_encoder::ColorType::Rgb)
        .ok()?;
    Some(out)
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8])
}

/// Read width, height, sample precision and component count from a JPEG header.
///
/// Returns `None` for anything unexpected rather than guessing. A wrong size
/// here is a picture drawn at the wrong aspect ratio in a printed job, and a
/// slow correct answer is available by decoding.
fn jpeg_header(bytes: &[u8]) -> Option<(u32, u32, u8, u8)> {
    let mut at = 2;
    while at + 3 < bytes.len() {
        if bytes[at] != 0xFF {
            return None;
        }
        let marker = bytes[at + 1];
        // Padding between markers is legal and carries no length.
        if marker == 0xFF {
            at += 1;
            continue;
        }
        // Standalone markers: no payload to skip.
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            at += 2;
            continue;
        }

        let length = u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]) as usize;
        // Every SOFn *except* the four that are not frame headers: DHT (C4),
        // JPG (C8) and DAC (CC) share the range and say nothing about size.
        let is_frame =
            (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if is_frame {
            // precision, height, width — heightfirst, which is the mistake to
            // make here and the reason this is one function with one test.
            let height = u16::from_be_bytes([*bytes.get(at + 5)?, *bytes.get(at + 6)?]);
            let width = u16::from_be_bytes([*bytes.get(at + 7)?, *bytes.get(at + 8)?]);
            let precision = *bytes.get(at + 4)?;
            let components = *bytes.get(at + 9)?;
            return (width > 0 && height > 0).then_some((
                u32::from(width),
                u32::from(height),
                precision,
                components,
            ));
        }
        at += 2 + length;
    }
    None
}

/// Deflate, which is what PDF's `/FlateDecode` reads.
pub(crate) fn deflate(bytes: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing to a `Vec` cannot fail, and neither can finishing it.
    let _ = encoder.write_all(bytes);
    encoder.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest real JPEG: a 1×1 white pixel.
    fn tiny_jpeg() -> Vec<u8> {
        let mut out = Vec::new();
        let image = image::RgbImage::from_pixel(3, 7, image::Rgb([255, 255, 255]));
        image::DynamicImage::ImageRgb8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .expect("encode");
        out
    }

    fn a_conversion() -> Conversion {
        // The screen profile, which is RGB. That makes this a test of the
        // *path* rather than of any press's numbers — the numbers need a real
        // CMYK profile, and `tools/vendor-profiles.py` has never been run.
        tessera_color::managed::OutputProfile::screen()
            .expect("a profile")
            .ink_for_screen_colour(tessera_color::managed::Rendering::default())
            .expect("a conversion")
    }

    fn written(name: &str, image: image::DynamicImage) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("tessera-pdf-images");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(name);
        image.save(&path).expect("write");
        path
    }

    #[test]
    fn every_placeable_format_reaches_the_page() {
        // The formats are enabled by a Cargo feature, which is the kind of
        // thing that silently stops being true: `default-features = false`
        // means a dropped flag does not fail to compile, it fails to open a
        // file. Each of these is written and read back through the same call
        // the exporter uses.
        //
        // TIFF is the one that matters — it is what a scanner writes and what
        // a repro house sends — and it was the reason the placeable list was
        // two formats long for so little cause.
        for (name, format) in [
            ("wide.tiff", image::ImageFormat::Tiff),
            ("wide.webp", image::ImageFormat::WebP),
            ("wide.bmp", image::ImageFormat::Bmp),
            ("wide.gif", image::ImageFormat::Gif),
            ("wide.png", image::ImageFormat::Png),
        ] {
            let source = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                6,
                4,
                image::Rgb([200, 40, 90]),
            ));
            let dir = std::env::temp_dir().join("tessera-pdf-images");
            std::fs::create_dir_all(&dir).expect("dir");
            let path = dir.join(name);
            source
                .save_with_format(&path, format)
                .unwrap_or_else(|e| panic!("{name} could not be written: {e}"));

            let ready =
                prepare(&path).unwrap_or_else(|e| panic!("{name} could not be read back: {e:?}"));
            assert_eq!(ready.width, 6, "{name} lost its width");
            assert_eq!(ready.height, 4, "{name} lost its height");
        }
    }

    /// A Photoshop file two pixels square: `mode` 3 for RGB or 4 for CMYK,
    /// its planes stored raw one after another, and no layers.
    fn a_psd(name: &str, mode: u16, planes: &[[u8; 4]]) -> std::path::PathBuf {
        let mut out = b"8BPS".to_vec();
        out.extend(1u16.to_be_bytes());
        out.extend([0; 6]);
        out.extend((planes.len() as u16).to_be_bytes());
        out.extend(2u32.to_be_bytes());
        out.extend(2u32.to_be_bytes());
        out.extend(8u16.to_be_bytes());
        out.extend(mode.to_be_bytes());
        out.extend([0; 12]); // no palette, resources, layers or masks
        out.extend(0u16.to_be_bytes());
        for plane in planes {
            out.extend(plane);
        }
        let dir = std::env::temp_dir().join("tessera-pdf-images");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(name);
        std::fs::write(&path, out).expect("write");
        path
    }

    #[test]
    fn a_photoshop_file_reaches_the_page() {
        let path = a_psd(
            "placed.psd",
            3,
            &[[255, 0, 0, 9], [0, 255, 0, 9], [0, 0, 255, 9]],
        );
        let ready = prepare(&path).expect("reads");
        assert_eq!((ready.width, ready.height, ready.space), (2, 2, Space::Rgb));
        assert_eq!(
            inflate(&ready.data),
            [255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9],
            "the planes were not put back together into pixels"
        );
        assert!(ready.alpha.is_none());
    }

    #[test]
    fn a_cmyk_photoshop_file_keeps_its_own_inks() {
        // The retoucher's numbers, not a round trip through RGB: that would
        // move every one of them, and put the black a retoucher kept on one
        // plate back into all four.
        let path = a_psd(
            "inks.psd",
            4,
            &[
                [0, 255, 255, 128],
                [255, 255, 255, 128],
                [255, 255, 255, 128],
                [255, 255, 0, 128],
            ],
        );
        let ready = to_cmyk(&path, &a_conversion()).expect("reads");
        assert_eq!(ready.space, Space::Cmyk);
        assert_eq!(
            inflate(&ready.data),
            [
                255, 0, 0, 0, // cyan
                0, 0, 0, 0, // paper
                0, 0, 0, 255, // black alone
                127, 127, 127, 127,
            ]
        );
        assert!(ready.alpha.is_none());

        // An RGB one is converted, like any photograph.
        let rgb = a_psd(
            "rgb-inks.psd",
            3,
            &[[255, 0, 0, 9], [0, 255, 0, 9], [0, 0, 255, 9]],
        );
        let ready = to_cmyk(&rgb, &a_conversion()).expect("reads");
        assert_eq!((ready.space, inflate(&ready.data).len()), (Space::Cmyk, 16));
    }

    #[test]
    fn converting_gives_four_components_a_pixel() {
        // Three would be an RGB image wearing a CMYK label, which a press would
        // read as a third of the picture.
        let path = written(
            "convert.png",
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                4,
                5,
                image::Rgb([10, 120, 200]),
            )),
        );
        let ready = to_cmyk(&path, &a_conversion()).expect("convert");

        assert_eq!(ready.space, Space::Cmyk);
        let raw = inflate(&ready.data);
        assert_eq!(raw.len(), 4 * 4 * 5, "not four components a pixel");
    }

    #[test]
    fn a_jpeg_cannot_be_passed_through_when_it_has_to_be_converted() {
        // `/DCTDecode` carries the file's own bytes and those bytes are RGB.
        // Converting means decoding, so the pass-through that makes an RGB
        // export cheap is exactly what a CMYK export cannot have.
        let bytes = tiny_jpeg();
        let dir = std::env::temp_dir().join("tessera-pdf-images");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("converted.jpg");
        std::fs::write(&path, &bytes).expect("write");

        let ready = to_cmyk(&path, &a_conversion()).expect("convert");
        assert_eq!(ready.coding, Coding::Flate, "the RGB bytes were passed on");
        assert_ne!(ready.data, bytes);
    }

    #[test]
    fn alpha_survives_conversion() {
        // It is coverage, not colour, and has nothing to do with which inks the
        // picture is made of. Dropping it here would composite a cut-out onto
        // black in exactly the export that goes to a press.
        let mut image = image::RgbaImage::from_pixel(3, 3, image::Rgba([9, 9, 9, 255]));
        image.put_pixel(0, 0, image::Rgba([9, 9, 9, 0]));
        let path = written("convert-alpha.png", image::DynamicImage::ImageRgba8(image));

        let ready = to_cmyk(&path, &a_conversion()).expect("convert");
        assert!(ready.is_transparent(), "the alpha channel was dropped");
    }

    /// Undo `deflate`, so a test can look at the samples that were written.
    fn inflate(bytes: &[u8]) -> Vec<u8> {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(bytes)
            .read_to_end(&mut out)
            .expect("inflate");
        out
    }

    #[test]
    fn a_jpeg_header_gives_width_and_height_the_right_way_round() {
        // A JPEG frame header states **height before width**, which is the
        // mistake to make here — and making it draws every picture at the wrong
        // aspect ratio in a printed job.
        let bytes = tiny_jpeg();
        assert_eq!(jpeg_header(&bytes), Some((3, 7, 8, 3)));
    }

    #[test]
    fn a_jpeg_is_passed_through_rather_than_re_encoded() {
        // `/DCTDecode` is JPEG. Handing over the file's own bytes is smaller
        // than anything this could produce and exactly as good as the original.
        let bytes = tiny_jpeg();
        let dir = std::env::temp_dir().join("tessera-pdf-images");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("passthrough.jpg");
        std::fs::write(&path, &bytes).expect("write");

        let ready = prepare(&path).expect("prepare");
        assert_eq!(ready.coding, Coding::Jpeg);
        assert_eq!(ready.data, bytes, "the bytes were not the file's own");
        assert!(!ready.is_transparent());
    }

    #[test]
    fn transparency_becomes_a_separate_mask() {
        // PDF has no RGBA. Dropping the alpha would composite a cut-out
        // photograph onto black.
        let dir = std::env::temp_dir().join("tessera-pdf-images");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("cutout.png");

        let mut image = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
        image.put_pixel(0, 0, image::Rgba([10, 20, 30, 0]));
        image.save(&path).expect("write");

        let ready = prepare(&path).expect("prepare");
        assert_eq!(ready.coding, Coding::Flate);
        assert!(ready.is_transparent(), "the alpha channel was dropped");
    }

    #[test]
    fn an_opaque_image_carries_no_mask() {
        // A fully opaque soft mask is a second image the size of the first that
        // changes nothing, and it would double the file.
        let dir = std::env::temp_dir().join("tessera-pdf-images");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("opaque.png");

        image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .expect("write");

        let ready = prepare(&path).expect("prepare");
        assert!(!ready.is_transparent(), "an all-opaque mask was written");
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_error_rather_than_a_panic() {
        // Preflight has already reported it. The export goes on without the
        // picture, because a job with a broken link still has to be proofable.
        let dir = std::env::temp_dir().join("tessera-pdf-images");
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("not-an-image.txt");
        std::fs::write(&path, b"this is not a picture").expect("write");

        assert!(prepare(&path).is_err());
    }

    #[test]
    fn deflated_bytes_are_smaller_than_what_went_in() {
        // Not a compression benchmark: a check that the encoder ran at all. An
        // empty result here would write a valid PDF holding a blank image.
        let flat = vec![7u8; 4096];
        let packed = deflate(&flat);
        assert!(!packed.is_empty());
        assert!(packed.len() < flat.len());
    }
}
