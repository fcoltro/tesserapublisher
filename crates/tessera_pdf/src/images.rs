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

/// How the bytes are compressed, which is the same thing as which PDF filter
/// reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coding {
    /// The file's own JPEG bytes, untouched.
    Jpeg,
    /// Raw samples, deflated.
    Flate,
}

/// One image, ready to be written.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub width: u32,
    pub height: u32,
    pub coding: Coding,
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
        data: deflate(&colour),
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
fn deflate(bytes: &[u8]) -> Vec<u8> {
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
