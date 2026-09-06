//! Decoded artwork, kept so it is decoded once rather than every frame.
//!
//! A page of photographs is tens of megabytes of JPEG. Decoding them on each
//! redraw would make panning unusable, so what is decoded is kept — and kept
//! **keyed on the file's modification time as well as its path**, so replacing
//! the file on disk shows the new artwork without anybody being asked to
//! reload. That is the whole of "watch the link update".
//!
//! The cache is bounded by total pixels rather than by entry count. Ten
//! thumbnails and one poster are very different amounts of memory, and a limit
//! that cannot tell them apart is a limit that either wastes room or thrashes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

/// What identifies a decoded image.
///
/// The modification time is in the key on purpose: a file replaced on disk is
/// a different image with the same name, and a cache keyed on the path alone
/// would happily show the old one forever.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    modified: Option<u64>,
}

/// Artwork decoded and ready to draw.
#[derive(Clone)]
pub struct Decoded {
    pub image: ImageData,
    /// The size in pixels, which is what effective PPI is worked out from.
    pub pixels: (u32, u32),
}

/// How many pixels the cache will hold before it starts letting go.
///
/// Sixty-four million is around 256MB of RGBA — a few large photographs, or a
/// great many thumbnails.
const BUDGET: usize = 64_000_000;

#[derive(Default)]
pub struct Images {
    held: HashMap<Key, Decoded>,
    /// Least-recently-used last. A `Vec` rather than something cleverer
    /// because the cache holds tens of entries, not thousands.
    order: Vec<Key>,
    pixels: usize,
    decodes: u64,
}

impl Images {
    pub fn new() -> Self {
        Self::default()
    }

    /// The artwork at `path`, decoding it if this is the first time it has
    /// been asked for since it last changed.
    ///
    /// `None` for a file that is not there or cannot be read as an image —
    /// which is a fact about the document to report, not an error to stop for.
    /// A page with one broken link still draws.
    pub fn get(&mut self, path: &Path) -> Option<&Decoded> {
        let modified = std::fs::metadata(path)
            .ok()
            .and_then(|m| tessera_document::links::modified_seconds(&m));
        let key = Key {
            path: path.to_path_buf(),
            modified,
        };

        if self.held.contains_key(&key) {
            self.touch(&key);
            return self.held.get(&key);
        }

        let decoded = decode(path)?;
        self.pixels += decoded.pixels.0 as usize * decoded.pixels.1 as usize;
        self.decodes += 1;
        self.held.insert(key.clone(), decoded);
        self.order.push(key.clone());
        self.evict_to_budget();
        self.held.get(&key)
    }

    /// How many times a file has actually been read and decoded.
    ///
    /// For tests, and for a diagnostics panel later: the number that says
    /// whether the cache is working.
    pub fn decodes(&self) -> u64 {
        self.decodes
    }

    pub fn held(&self) -> usize {
        self.held.len()
    }

    fn touch(&mut self, key: &Key) {
        if let Some(at) = self.order.iter().position(|k| k == key) {
            let key = self.order.remove(at);
            self.order.push(key);
        }
    }

    fn evict_to_budget(&mut self) {
        while self.pixels > BUDGET && self.order.len() > 1 {
            let oldest = self.order.remove(0);
            if let Some(gone) = self.held.remove(&oldest) {
                self.pixels = self
                    .pixels
                    .saturating_sub(gone.pixels.0 as usize * gone.pixels.1 as usize);
            }
        }
    }
}

/// Read and decode one file.
fn decode(path: &Path) -> Option<Decoded> {
    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let decoded = reader.decode().ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());

    Some(Decoded {
        image: ImageData {
            data: Blob::new(std::sync::Arc::new(rgba.into_raw())),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width,
            height,
        },
        pixels: (width, height),
    })
}

/// The resolution artwork is actually reproduced at, in pixels per inch.
///
/// **Effective**, not natural: a 300ppi photograph scaled to twice its size is
/// a 150ppi photograph, and it is the effective figure a printer cares about.
/// Returns `None` for artwork drawn at no size, which has no resolution rather
/// than an infinite one.
pub fn effective_ppi(pixels: (u32, u32), drawn: (f64, f64)) -> Option<(f64, f64)> {
    if drawn.0 <= 0.0 || drawn.1 <= 0.0 {
        return None;
    }
    // 72 points to the inch.
    Some((
        f64::from(pixels.0) / (drawn.0 / 72.0),
        f64::from(pixels.1) / (drawn.1 / 72.0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny PNG written to a temporary file.
    fn a_png(name: &str, size: u32) -> PathBuf {
        let path = std::env::temp_dir().join(format!("tessera-img-{name}.png"));
        let buffer = image::RgbaImage::from_pixel(size, size, image::Rgba([10, 20, 30, 255]));
        buffer.save(&path).expect("write a png");
        path
    }

    #[test]
    fn artwork_is_decoded_once_however_often_it_is_asked_for() {
        // A page of photographs decoded on every redraw would make panning
        // unusable.
        let path = a_png("once", 8);
        let mut images = Images::new();

        for _ in 0..5 {
            assert!(images.get(&path).is_some());
        }
        assert_eq!(images.decodes(), 1);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_replaced_on_disk_is_decoded_again() {
        // "Replace the file and watch the link update", and the reason the
        // modification time is in the key.
        let path = a_png("replaced", 8);
        let mut images = Images::new();
        assert_eq!(images.get(&path).expect("decoded").pixels, (8, 8));

        // Rewritten larger, and given a different modification time.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let bigger = image::RgbaImage::from_pixel(16, 16, image::Rgba([1, 2, 3, 255]));
        bigger.save(&path).expect("write a png");

        assert_eq!(images.get(&path).expect("decoded").pixels, (16, 16));
        assert_eq!(images.decodes(), 2);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_that_is_not_there_is_reported_rather_than_fatal() {
        // A page with one broken link still draws.
        let mut images = Images::new();
        assert!(images.get(Path::new("nothing-of-this-name.png")).is_none());
        assert_eq!(images.decodes(), 0);
    }

    #[test]
    fn a_file_that_is_not_an_image_is_reported_rather_than_fatal() {
        let path = std::env::temp_dir().join("tessera-img-not-an-image.png");
        std::fs::write(&path, b"this is not a png").expect("write");

        let mut images = Images::new();
        assert!(images.get(&path).is_none());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn two_files_are_held_side_by_side() {
        let a = a_png("side-a", 8);
        let b = a_png("side-b", 8);
        let mut images = Images::new();
        images.get(&a);
        images.get(&b);

        assert_eq!(images.held(), 2);
        assert_eq!(images.decodes(), 2);

        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    // --- effective resolution ------------------------------------------------

    #[test]
    fn artwork_at_its_natural_size_reports_seventy_two_ppi() {
        // A point is a 72nd of an inch, so one pixel per point is 72ppi.
        let at = effective_ppi((100, 100), (100.0, 100.0)).expect("a resolution");
        assert!((at.0 - 72.0).abs() < 1e-9);
    }

    #[test]
    fn scaling_artwork_up_halves_its_resolution() {
        // **The number a printer cares about**: a 300ppi photograph at twice
        // its size is a 150ppi photograph.
        let small = effective_ppi((600, 600), (144.0, 144.0)).expect("a resolution");
        let large = effective_ppi((600, 600), (288.0, 288.0)).expect("a resolution");
        assert!((small.0 - 2.0 * large.0).abs() < 1e-9);
    }

    #[test]
    fn a_three_hundred_ppi_placement_reports_three_hundred() {
        // 300ppi means 300 pixels to the inch, and an inch is 72 points.
        let at = effective_ppi((300, 300), (72.0, 72.0)).expect("a resolution");
        assert!((at.0 - 300.0).abs() < 1e-9, "got {}", at.0);
    }

    #[test]
    fn a_stretched_placement_reports_two_different_resolutions() {
        // Stretching is not proportional, so the two axes really do differ and
        // a single figure would hide it.
        let at = effective_ppi((300, 300), (72.0, 144.0)).expect("a resolution");
        assert!((at.0 - 300.0).abs() < 1e-9);
        assert!((at.1 - 150.0).abs() < 1e-9);
    }

    #[test]
    fn artwork_drawn_at_no_size_has_no_resolution_rather_than_an_infinite_one() {
        assert!(effective_ppi((300, 300), (0.0, 72.0)).is_none());
        assert!(effective_ppi((300, 300), (72.0, 0.0)).is_none());
    }
}
