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
    /// The size band this was decoded for, or `None` for the original.
    ///
    /// In the key because a picture wanted small and the same picture wanted
    /// large are two different bitmaps, and holding one under the other’s name
    /// would either draw a blurred original or decode a big one to show a thumb.
    bucket: Option<u32>,
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
    from_proxy: u64,
    /// Where proxies are written, or `None` to write none.
    ///
    /// Held rather than looked up on every read, so the cache can be pointed
    /// somewhere else — a portable install, a test — without a global to set and
    /// without every caller passing a path it does not care about.
    directory: Option<PathBuf>,
}

impl Images {
    /// A cache that keeps proxies where this platform puts caches.
    pub fn new() -> Self {
        Self {
            directory: crate::proxies::directory(),
            ..Self::default()
        }
    }

    /// A cache that keeps its proxies in `directory`, or nowhere for `None`.
    pub fn keeping_proxies_in(directory: Option<PathBuf>) -> Self {
        Self {
            directory,
            ..Self::default()
        }
    }

    /// The artwork at `path`, decoding it if this is the first time it has
    /// been asked for since it last changed.
    ///
    /// `None` for a file that is not there or cannot be read as an image —
    /// which is a fact about the document to report, not an error to stop for.
    /// A page with one broken link still draws.
    pub fn get(&mut self, path: &Path) -> Option<&Decoded> {
        self.at_size(path, None)
    }

    /// The artwork at `path`, at no more than `longest_edge` points across.
    ///
    /// The size is what lets the **disk** cache earn its keep: a proxy at that
    /// size is read back on a cold start instead of the original being decoded
    /// again. `None` for the size asks for the original, which is what anything
    /// needing real pixels — a resolution report, an export — must have.
    pub fn at_size(&mut self, path: &Path, longest_edge: Option<u32>) -> Option<&Decoded> {
        let modified = std::fs::metadata(path)
            .ok()
            .and_then(|m| tessera_document::links::modified_seconds(&m));
        let bucket = longest_edge.map(crate::proxies::bucket);
        let key = Key {
            path: path.to_path_buf(),
            modified,
            bucket,
        };

        if self.held.contains_key(&key) {
            self.touch(&key);
            return self.held.get(&key);
        }

        // The disk cache, before the decoder. This is the whole point of having
        // one: on a cold start the pixels are already there, small, and need no
        // JPEG pulled apart to reach them.
        let decoded = self.read_proxy(path, modified, bucket).or_else(|| {
            let decoded = decode(path, bucket)?;
            self.write_proxy(path, modified, bucket, &decoded);
            Some(decoded)
        })?;

        self.pixels += decoded.pixels.0 as usize * decoded.pixels.1 as usize;
        self.decodes += 1;
        self.held.insert(key.clone(), decoded);
        self.order.push(key.clone());
        self.evict_to_budget();
        self.held.get(&key)
    }

    /// Read a proxy back, when one was asked for and one is there.
    ///
    /// Takes `&mut self` because a hit is counted, which is what makes the disk
    /// cache observable at all.
    ///
    /// Never for the original size: a proxy is a downscale, and writing the full
    /// pixels of every photograph into a cache directory would be a copy of the
    /// user’s picture library.
    fn read_proxy(
        &mut self,
        path: &Path,
        modified: Option<u64>,
        bucket: Option<u32>,
    ) -> Option<Decoded> {
        let bucket = bucket?;
        let file = self
            .directory
            .as_ref()?
            .join(crate::proxies::name_for(path, modified, bucket));
        let proxy = crate::proxies::read(&file)?;
        self.from_proxy += 1;
        Some(Decoded {
            image: to_image(proxy.pixels, proxy.width, proxy.height),
            pixels: (proxy.width, proxy.height),
        })
    }

    /// Write a proxy out, so tomorrow’s first draw is a read.
    ///
    /// Failure is silence: a read-only cache directory or a full disk makes
    /// Tessera slower, not broken.
    fn write_proxy(
        &self,
        path: &Path,
        modified: Option<u64>,
        bucket: Option<u32>,
        decoded: &Decoded,
    ) {
        let Some(bucket) = bucket else { return };
        let Some(directory) = self.directory.clone() else {
            return;
        };
        let proxy = crate::proxies::Proxy {
            width: decoded.pixels.0,
            height: decoded.pixels.1,
            pixels: decoded.image.data.as_ref().to_vec(),
        };
        let file = directory.join(crate::proxies::name_for(path, modified, bucket));
        if crate::proxies::write(&file, &proxy) {
            // Only when something was added, because that is the only time the
            // directory can have grown past its budget.
            crate::proxies::evict(&directory, crate::proxies::BUDGET);
        }
    }

    /// How many times a proxy was read back rather than a file decoded.
    ///
    /// The number that says whether the disk cache is working, as `decodes` is
    /// for the one in memory.
    pub fn from_proxy(&self) -> u64 {
        self.from_proxy
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

/// Read and decode one file, optionally downscaled.
///
/// Downscaled here rather than by whoever asked, so that the large bitmap exists
/// for as long as it takes to shrink it and no longer: decoding at full size and
/// handing it on would put a 40-megapixel image in the cache to draw a thumbnail
/// from.
fn decode(path: &Path, longest_edge: Option<u32>) -> Option<Decoded> {
    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let decoded = reader.decode().ok()?;

    // Only ever down. Scaling a small picture up to fill a bucket would make a
    // blurred copy of it and charge memory for the blur.
    let scaled = match longest_edge {
        Some(edge) if decoded.width().max(decoded.height()) > edge => decoded.thumbnail(edge, edge),
        _ => decoded,
    };

    let rgba = scaled.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Some(Decoded {
        image: to_image(rgba.into_raw(), width, height),
        pixels: (width, height),
    })
}

/// Wrap raw RGBA bytes as something vello can draw.
fn to_image(pixels: Vec<u8>, width: u32, height: u32) -> ImageData {
    ImageData {
        data: Blob::new(std::sync::Arc::new(pixels)),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    }
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

    #[test]
    fn asking_for_a_size_gets_a_picture_no_larger_than_that() {
        // A thumbnail wants a thumbnail, not a large photograph shrunk on every
        // frame.
        let path = a_png("downscaled", 512);
        let mut images = Images::new();

        let decoded = images.at_size(&path, Some(64)).expect("decoded");
        assert!(
            decoded.pixels.0 <= 128 && decoded.pixels.1 <= 128,
            "got {:?}",
            decoded.pixels
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_small_picture_is_never_scaled_up_to_fill_a_bucket() {
        // That would make a blurred copy and charge memory for the blur.
        let path = a_png("small", 16);
        let mut images = Images::new();
        assert_eq!(
            images.at_size(&path, Some(1024)).expect("decoded").pixels,
            (16, 16)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn one_picture_wanted_at_two_sizes_is_two_bitmaps() {
        // Holding one under the other’s name would either draw a blurred
        // original or decode a big one to show a thumbnail.
        let path = a_png("two-sizes", 512);
        let mut images = Images::new();
        images.at_size(&path, Some(64));
        images.at_size(&path, Some(400));
        assert_eq!(images.held(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_original_is_still_reachable_for_anything_that_needs_real_pixels() {
        // A resolution report and an export must have the file’s own pixels, not
        // a proxy’s.
        let path = a_png("original", 300);
        let mut images = Images::new();
        assert_eq!(images.get(&path).expect("decoded").pixels, (300, 300));
        let _ = std::fs::remove_file(&path);
    }

    /// A cache whose proxies go somewhere the test can reason about.
    fn a_cache_in(name: &str) -> (Images, PathBuf) {
        let at = std::env::temp_dir().join(format!("tessera-proxies-{name}"));
        let _ = std::fs::remove_dir_all(&at);
        (Images::keeping_proxies_in(Some(at.clone())), at)
    }

    #[test]
    fn a_proxy_written_today_is_read_back_tomorrow_rather_than_decoded_again() {
        // **What the disk cache is for.** The in-memory one already stops a
        // decode per frame; this stops one per restart, which is the wait a
        // person actually notices when they open a picture-heavy document.
        let path = a_png("cold-start", 512);
        let (mut today, directory) = a_cache_in("cold-start");

        assert!(today.at_size(&path, Some(200)).is_some());
        assert_eq!(today.decodes(), 1);
        assert_eq!(today.from_proxy(), 0, "there was nothing to read yet");

        // A new session: nothing in memory, everything still on disk.
        let mut tomorrow = Images::keeping_proxies_in(Some(directory.clone()));
        assert!(tomorrow.at_size(&path, Some(200)).is_some());
        assert_eq!(
            tomorrow.from_proxy(),
            1,
            "the file was decoded again instead of the proxy being read"
        );

        let _ = std::fs::remove_dir_all(&directory);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_replaced_on_disk_is_not_served_from_yesterdays_proxy() {
        // The modification time is in the proxy’s name for exactly this: a
        // cache that showed last week’s photograph would be worse than no cache.
        let path = a_png("proxy-replaced", 256);
        let (mut first, directory) = a_cache_in("proxy-replaced");
        first.at_size(&path, Some(200));

        std::thread::sleep(std::time::Duration::from_millis(1100));
        image::RgbaImage::from_pixel(64, 64, image::Rgba([9, 9, 9, 255]))
            .save(&path)
            .expect("write a png");

        let mut later = Images::keeping_proxies_in(Some(directory.clone()));
        let decoded = later.at_size(&path, Some(200)).expect("decoded");
        assert_eq!(decoded.pixels, (64, 64), "the old proxy was served");
        assert_eq!(later.from_proxy(), 0);

        let _ = std::fs::remove_dir_all(&directory);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_original_size_is_never_written_to_disk() {
        // Writing the full pixels of every photograph into a cache directory
        // would be a copy of the user’s picture library.
        let path = a_png("no-proxy-for-original", 64);
        let (mut images, directory) = a_cache_in("no-proxy-for-original");
        images.get(&path);

        let count = std::fs::read_dir(&directory)
            .map(|entries| entries.flatten().count())
            .unwrap_or(0);
        assert_eq!(count, 0, "a full-size proxy was written");

        let _ = std::fs::remove_dir_all(&directory);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_cache_that_cannot_be_written_makes_tessera_slower_rather_than_broken() {
        let path = a_png("no-cache-dir", 128);
        let mut images = Images::keeping_proxies_in(None);
        assert!(images.at_size(&path, Some(64)).is_some());
        assert_eq!(images.from_proxy(), 0);
        let _ = std::fs::remove_file(&path);
    }
}
