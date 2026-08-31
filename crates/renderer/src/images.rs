//! Decoding and caching for linked raster images.
//!
//! Two things matter here. Decoding is expensive, so a decoded image is kept
//! and reused across frames rather than re-read on every repaint. And print
//! images are enormous — a full-page 300 PPI photograph is tens of megapixels,
//! far more than a screen can show — so what gets cached is a *proxy*: the
//! image downscaled to something the canvas can actually resolve. The link
//! still points at the original file, which is what export will read.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

/// Longest edge, in pixels, kept for on-canvas display.
///
/// Sized for a generous zoom on a high-DPI display. Beyond this the extra
/// pixels cannot be resolved on screen but still cost memory and upload
/// bandwidth on every frame.
const PROXY_MAX_EDGE: u32 = 2048;

/// What is known about a linked file right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkStatus {
    /// The file is present and unchanged since it was linked.
    Ok,
    /// The file exists but has been written since it was cached.
    Modified,
    /// The path no longer resolves to a readable file.
    Missing,
}

/// A decoded image plus the file state it was decoded from.
struct CacheEntry {
    image: Arc<ImageData>,
    modified: Option<SystemTime>,
    /// True when the cached pixels are a downscale of the original.
    is_proxy: bool,
}

/// Decoded images, keyed by absolute path.
#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<String, CacheEntry>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the decoded proxy for `path`, decoding it if necessary.
    ///
    /// A file rewritten on disk is re-read rather than served stale, which is
    /// what makes "edit the photo in another application and see it update"
    /// work without an explicit relink.
    pub fn get(&mut self, path: &str) -> Option<Arc<ImageData>> {
        let modified = file_modified(path);

        if let Some(entry) = self.entries.get(path) {
            if entry.modified == modified {
                return Some(entry.image.clone());
            }
        }

        let (image, is_proxy) = decode_proxy(path)?;
        let image = Arc::new(image);
        self.entries.insert(
            path.to_string(),
            CacheEntry {
                image: image.clone(),
                modified,
                is_proxy,
            },
        );
        Some(image)
    }

    /// Whether the cached pixels for `path` are a downscale of the original.
    pub fn is_proxy(&self, path: &str) -> bool {
        self.entries.get(path).is_some_and(|e| e.is_proxy)
    }

    /// How the file at `path` compares to what was cached from it.
    pub fn status(&self, path: &str) -> LinkStatus {
        if !Path::new(path).is_file() {
            return LinkStatus::Missing;
        }
        match self.entries.get(path) {
            // Never loaded, but present and readable.
            None => LinkStatus::Ok,
            Some(entry) if entry.modified == file_modified(path) => LinkStatus::Ok,
            Some(_) => LinkStatus::Modified,
        }
    }

    pub fn forget(&mut self, path: &str) {
        self.entries.remove(path);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn file_modified(path: &str) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Reads an image's pixel dimensions without decoding its contents.
///
/// Used when placing a file, where only the aspect ratio is needed to size the
/// frame — decoding a 200 MB TIFF to learn it is 4000 pixels wide is waste.
pub fn probe_dimensions(path: &str) -> Result<(u32, u32), String> {
    image::image_dimensions(path).map_err(|e| format!("could not read {path}: {e}"))
}

/// Decodes `path` to RGBA8, downscaling to the proxy limit when oversized.
///
/// Returns the image and whether it was downscaled.
fn decode_proxy(path: &str) -> Option<(ImageData, bool)> {
    let decoded = image::open(path).ok()?;
    let (width, height) = (decoded.width(), decoded.height());
    let longest = width.max(height);

    let (rgba, is_proxy) = if longest > PROXY_MAX_EDGE {
        let scale = PROXY_MAX_EDGE as f32 / longest as f32;
        let target_w = ((width as f32 * scale).round() as u32).max(1);
        let target_h = ((height as f32 * scale).round() as u32).max(1);
        // Triangle filtering is a deliberate middle ground: nearest would alias
        // badly on downscale, and Lanczos costs more than a screen proxy earns.
        (
            decoded
                .resize_exact(target_w, target_h, image::imageops::FilterType::Triangle)
                .to_rgba8(),
            true,
        )
    } else {
        (decoded.to_rgba8(), false)
    };

    let (w, h) = (rgba.width(), rgba.height());
    Some((
        ImageData {
            data: Blob::new(Arc::new(rgba.into_raw())),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: w,
            height: h,
        },
        is_proxy,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_reports_missing() {
        let cache = ImageCache::new();
        assert_eq!(cache.status("no/such/file.png"), LinkStatus::Missing);
    }

    #[test]
    fn a_missing_file_decodes_to_nothing_rather_than_panicking() {
        let mut cache = ImageCache::new();
        assert!(cache.get("no/such/file.png").is_none());
        assert!(cache.is_empty(), "a failed load must not leave an entry");
    }

    #[test]
    fn probing_a_missing_file_is_an_error_not_a_panic() {
        assert!(probe_dimensions("no/such/file.png").is_err());
    }
}
