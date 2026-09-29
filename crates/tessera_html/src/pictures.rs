//! Pictures as they are seen in their frames.
//!
//! **The frame's view, not the file.** A picture placed large and cropped
//! by its frame shows the part the frame shows, at the size it is shown —
//! InDesign's "preserve appearance from layout". So the export renders what
//! the frame holds, at the resolution asked for, rather than copying the
//! original and hoping a browser crops it the same way.

use std::path::Path;

use tessera_document::document::Document;
use tessera_document::ids::FrameId;
use tessera_document::nodes::FrameKind;
use tessera_geometry::DocPoint;

/// How pictures are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageFormat {
    /// JPEG for a picture with no transparency, PNG for one with.
    #[default]
    Automatic,
    Png,
    Jpeg,
}

/// A picture made for a page.
pub struct Picture {
    /// The file's own name, for its file and its alternative text.
    pub stem: String,
    pub extension: &'static str,
    pub bytes: Vec<u8>,
    /// The size it is shown at, in CSS pixels.
    pub width: f64,
    pub height: f64,
}

/// The most pixels a side, so a poster at a high resolution does not ask
/// for gigabytes.
const LARGEST: f64 = 6000.0;

/// What `frame` shows, rendered at `ppi`; `None` for a frame showing
/// nothing, or a file that is gone or will not read.
pub fn picture(doc: &Document, frame: FrameId, ppi: f64, format: ImageFormat) -> Option<Picture> {
    let f = doc.frame(frame)?;
    let FrameKind::Graphic {
        placed: Some(placement),
    } = &f.kind
    else {
        return None;
    };
    let link = doc.links.get(placement.link)?;
    let path = link.path.as_path();
    let (natural_w, natural_h) = link.natural;
    if natural_w <= 0.0 || natural_h <= 0.0 {
        return None;
    }
    let ppi = ppi.clamp(36.0, 1200.0);
    let dots = ppi / 72.0;
    let width = ((f.bounds.width * dots).round()).clamp(1.0, LARGEST) as u32;
    let height = ((f.bounds.height * dots).round()).clamp(1.0, LARGEST) as u32;

    // How much the placement enlarges the artwork: points of frame to a
    // unit of the artwork's natural size.
    let enlarged = placement.inner.determinant().abs().sqrt().max(1e-6);
    let wanted = natural_w.max(natural_h) * enlarged * dots;
    let source = source_pixels(path, link.pdf, wanted)?;

    // Every pixel of the frame, back through the placement to the artwork.
    let back = placement.inner.inverse();
    let (sw, sh) = (source.width(), source.height());
    let mut out = image::RgbaImage::new(width, height);
    for (x, y, pixel) in out.enumerate_pixels_mut() {
        let at = back.apply(DocPoint {
            x: (f64::from(x) + 0.5) / dots,
            y: (f64::from(y) + 0.5) / dots,
        });
        let sx = at.x / natural_w * f64::from(sw);
        let sy = at.y / natural_h * f64::from(sh);
        if sx >= 0.0 && sy >= 0.0 && sx < f64::from(sw) && sy < f64::from(sh) {
            *pixel = *source.get_pixel(sx as u32, sy as u32);
        }
    }

    let opaque = out.pixels().all(|p| p[3] == 255);
    let jpeg = match format {
        ImageFormat::Automatic => opaque,
        ImageFormat::Jpeg => true,
        ImageFormat::Png => false,
    };
    let mut bytes = Vec::new();
    let extension = if jpeg {
        // On white where it was clear: JPEG has no transparency.
        let flat = image::RgbImage::from_fn(width, height, |x, y| {
            let [r, g, b, a] = out.get_pixel(x, y).0;
            let over = |c: u8| {
                ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a)) + 127) / 255) as u8
            };
            image::Rgb([over(r), over(g), over(b)])
        });
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
            .encode_image(&flat)
            .ok()?;
        "jpg"
    } else {
        image::DynamicImage::ImageRgba8(out)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .ok()?;
        "png"
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "picture".to_owned());
    Some(Picture {
        stem,
        extension,
        bytes,
        width: f.bounds.width * 96.0 / 72.0,
        height: f.bounds.height * 96.0 / 72.0,
    })
}

/// The artwork's pixels, about `wanted` across its longest side: a drawing
/// or a PDF page rendered to it, a photograph brought down to it when it
/// has far more — sampling straight from a large photograph would alias.
fn source_pixels(
    path: &Path,
    pdf: tessera_document::links::PdfPage,
    wanted: f64,
) -> Option<image::RgbaImage> {
    use tessera_render::images;
    let converted = tessera_render::eps::effective(path);
    let path = converted.as_path();
    let edge = wanted.clamp(1.0, LARGEST) as u32;
    let rendered = |(rgba, (w, h)): (Vec<u8>, (u32, u32))| image::RgbaImage::from_raw(w, h, rgba);
    if images::is_svg(path) {
        return images::render_svg(path, edge).and_then(rendered);
    }
    if images::is_pdf(path) {
        return images::render_pdf_page(path, pdf, edge).and_then(rendered);
    }
    let decoded = if tessera_render::eps::is_eps(path) {
        let (rgba, (w, h)) = tessera_render::eps::preview(path)?;
        image::RgbaImage::from_raw(w, h, rgba)?
    } else if tessera_render::psd::is_psd(path) {
        let composite = tessera_render::psd::read(&std::fs::read(path).ok()?).ok()?;
        image::RgbaImage::from_raw(composite.width, composite.height, composite.rgba)?
    } else {
        image::open(path).ok()?.to_rgba8()
    };
    let longest = decoded.width().max(decoded.height());
    if f64::from(longest) > wanted * 1.5 {
        let scale = wanted / f64::from(longest);
        let w = ((f64::from(decoded.width()) * scale).round() as u32).max(1);
        let h = ((f64::from(decoded.height()) * scale).round() as u32).max(1);
        return Some(image::imageops::resize(
            &decoded,
            w,
            h,
            image::imageops::FilterType::Triangle,
        ));
    }
    Some(decoded)
}
