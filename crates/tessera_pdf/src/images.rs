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
pub fn prepare(path: &Path) -> Result<Prepared, Error> {
    if tessera_render::images::is_svg(path) {
        return prepare_svg(path);
    }
    let bytes = std::fs::read(path)?;

    if is_jpeg(&bytes) {
        // Dimensions from the file's own header rather than by decoding it:
        // reading a marker chain is a few hundred bytes of work where decoding
        // a 40-megapixel photograph is a second and a hundred megabytes.
        if let Some((width, height)) = jpeg_size(&bytes) {
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
        // A JPEG whose header cannot be walked is one this must not pass
        // through unread — falling through decodes it properly or fails.
    }

    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| Error::Unreadable(path.to_path_buf(), e.to_string()))?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();

    let mut colour = Vec::with_capacity((width * height * 3) as usize);
    let mut alpha = Vec::with_capacity((width * height) as usize);
    let mut any_transparent = false;
    for pixel in rgba.pixels() {
        colour.extend_from_slice(&pixel.0[..3]);
        alpha.push(pixel.0[3]);
        any_transparent |= pixel.0[3] != 255;
    }

    Ok(Prepared {
        width,
        height,
        coding: Coding::Flate,
        space: Space::Rgb,
        data: deflate(&colour),
        alpha: any_transparent.then(|| deflate(&alpha)),
    })
}

/// How finely a placed SVG is rendered for the page.
///
/// **Rasterised, and this is the compromise to know about.** Vector artwork
/// ought to reach a PDF as vectors; the crate that does that conversion
/// (`svg2pdf`) is built against `pdf-writer` 0.12 and this writes with 0.15,
/// so their types cannot meet. Until they line up, an SVG is rendered at a
/// resolution high enough that a press will not show it — 600 pixels per inch
/// is twice what a 300ppi photograph gets and is the usual number for line
/// work — and the limitation is written down here rather than discovered on a
/// proof.
const SVG_PPI: f64 = 600.0;

/// A placed SVG rendered for the page, at [`SVG_PPI`].
///
/// One function because both the RGB path and the CMYK one need the same
/// pixels; rendering twice would be slow and could disagree.
fn svg_rgba(path: &Path) -> Result<(Vec<u8>, u32, u32), Error> {
    let unreadable =
        |why: &str| Error::Unreadable(path.to_path_buf(), format!("not readable as SVG: {why}"));

    let (natural_w, natural_h) =
        tessera_render::images::svg_size(path).ok_or_else(|| unreadable("it does not parse"))?;
    if !(natural_w > 0.0 && natural_h > 0.0) {
        return Err(unreadable("the drawing has no size"));
    }

    // Points to pixels at the chosen resolution, asked of the same renderer the
    // screen uses so the proof and the page cannot disagree about the artwork.
    let longest = natural_w.max(natural_h);
    let edge = (longest / 72.0 * SVG_PPI)
        .round()
        .clamp(1.0, f64::from(u32::MAX)) as u32;

    let (rgba, (width, height)) = tessera_render::images::render_svg(path, edge)
        .ok_or_else(|| unreadable("it renders to nothing"))?;
    Ok((rgba, width, height))
}

/// Render a placed SVG into pixels for embedding.
fn prepare_svg(path: &Path) -> Result<Prepared, Error> {
    let (rgba, width, height) = svg_rgba(path)?;

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

    Ok(Prepared {
        width,
        height,
        coding: Coding::Flate,
        space: Space::Rgb,
        data: deflate(&colour),
        alpha: any_transparent.then(|| deflate(&alpha)),
    })
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
pub fn to_cmyk(path: &Path, conversion: &Conversion) -> Result<Prepared, Error> {
    // An SVG is rendered rather than decoded, and then converted like any other
    // picture: the inks a drawing prints in are the press's business, not the
    // drawing's.
    let rgba = if tessera_render::images::is_svg(path) {
        let (raw, w, h) = svg_rgba(path)?;
        image::RgbaImage::from_raw(w, h, raw)
            .ok_or_else(|| Error::Unreadable(path.to_path_buf(), "malformed rendering".into()))?
    } else {
        let bytes = std::fs::read(path)?;
        image::load_from_memory(&bytes)
            .map_err(|e| Error::Unreadable(path.to_path_buf(), e.to_string()))?
            .to_rgba8()
    };
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

    Ok(Prepared {
        width,
        height,
        coding: Coding::Flate,
        space: Space::Cmyk,
        data: deflate(&inks),
        alpha: any_transparent.then(|| deflate(&alpha)),
    })
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8])
}

/// Walk a JPEG's marker chain for the frame header that states its size.
///
/// Returns `None` for anything unexpected rather than guessing. A wrong size
/// here is a picture drawn at the wrong aspect ratio in a printed job, and a
/// slow correct answer is available by decoding.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
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
            return (width > 0 && height > 0).then_some((u32::from(width), u32::from(height)));
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
        assert_eq!(jpeg_size(&bytes), Some((3, 7)));
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
