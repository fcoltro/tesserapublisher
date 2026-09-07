//! Downscaled artwork, kept on disk so it survives a restart.
//!
//! [`crate::images`] decodes once per session. That is enough to make panning
//! usable, but the *first* draw of a page of 40-megapixel photographs still
//! costs seconds every time the application starts — and it costs them again
//! tomorrow, for exactly the same pixels.
//!
//! So what the screen actually needs is written out: a downscaled proxy, keyed
//! on the file, its modification time **and** the size asked for. Tomorrow's
//! first draw reads a small raw bitmap instead of decoding a large JPEG.
//!
//! **Raw RGBA rather than PNG.** A proxy is written once and read on every cold
//! start, so reading fast matters and writing small does not: re-encoding to PNG
//! on write and inflating on read would trade the one thing this exists to buy.
//! The files are the cache's own and are never handed to anyone, so nothing
//! outside has to be able to read them.
//!
//! A proxy is never authority. It is only for the screen: the PDF writer takes
//! the original bytes, and a missing, damaged or truncated proxy is discarded
//! and rebuilt rather than reported.

use std::path::{Path, PathBuf};

/// The magic and version at the head of a proxy file.
///
/// Version, not just magic: the layout may change, and a stale file from an
/// older build must be discarded rather than misread. Four bytes so the header
/// stays a fixed size.
const MAGIC: [u8; 4] = *b"TPX1";

/// Bytes before the pixels: magic, width, height.
const HEADER: usize = 4 + 4 + 4;

/// How much the cache directory may hold, in bytes.
///
/// A proxy is small; a thousand of them is not. Bounded so that a machine used
/// for a year does not quietly fill up with pictures nobody is laying out any
/// more.
pub const BUDGET: u64 = 512 * 1024 * 1024;

/// A downscaled bitmap, in RGBA.
pub struct Proxy {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Where proxies live on this platform, if the platform will say.
///
/// The *cache* directory rather than the config one, and that matters: a cache
/// is something the system may delete, and everything here can be rebuilt from
/// the original files. Putting it beside the preferences would ask the system to
/// back up derived data and would lose settings if it ever cleared it.
pub fn directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);

    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Caches"));

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let root = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")));

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let name = "Tessera";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let name = "tessera";

    root.map(|root| root.join(name).join("proxies"))
}

/// The file name a proxy is stored under.
///
/// A hash of the path, the modification time and the size asked for. The
/// modification time is in the name for the same reason it is in the in-memory
/// cache's key: a file replaced on disk is a different picture with the same
/// name, and a proxy keyed on the path alone would show the old one for ever.
///
/// A hash rather than the path itself because a path contains separators,
/// colons and characters no filesystem agrees about, and because a deep path is
/// longer than a file name may be.
pub fn name_for(path: &Path, modified: Option<u64>, longest_edge: u32) -> String {
    // FNV-1a, 64-bit. Not a cryptographic hash and does not need to be: a
    // collision costs one wrong proxy on screen, and the alternative is a
    // dependency for something nothing depends on.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    };
    eat(path.to_string_lossy().as_bytes());
    eat(&modified.unwrap_or(0).to_le_bytes());
    eat(&longest_edge.to_le_bytes());

    format!("{hash:016x}.tpx")
}

/// Read a proxy, if there is a good one.
///
/// `None` for a file that is not there, is too short, carries the wrong magic,
/// or whose pixel count does not match its own header. Every one of those is a
/// cache to rebuild rather than a fault to report: a proxy is derived data, and
/// the original is still on disk.
pub fn read(file: &Path) -> Option<Proxy> {
    let bytes = std::fs::read(file).ok()?;
    if bytes.len() < HEADER || bytes[..4] != MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[8..12].try_into().ok()?);

    let wanted = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    let pixels = bytes.get(HEADER..)?;
    if pixels.len() != wanted {
        // Truncated, or written by something that thought differently about the
        // layout. Either way it is not this proxy.
        return None;
    }

    Some(Proxy {
        width,
        height,
        pixels: pixels.to_vec(),
    })
}

/// Write a proxy, and say nothing if it cannot be written.
///
/// A cache that cannot be written is slower, not broken: a read-only cache
/// directory, a full disk or a locked file must leave the application working.
/// The one thing it must not do is leave a half-written file that a later read
/// would trust, which is why the pixels and the header go out together.
pub fn write(file: &Path, proxy: &Proxy) -> bool {
    if proxy.pixels.len() != (proxy.width as usize) * (proxy.height as usize) * 4 {
        // Refusing to write a proxy that disagrees with itself, rather than
        // writing one that `read` will throw away.
        return false;
    }
    if let Some(parent) = file.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }

    let mut bytes = Vec::with_capacity(HEADER + proxy.pixels.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&proxy.width.to_le_bytes());
    bytes.extend_from_slice(&proxy.height.to_le_bytes());
    bytes.extend_from_slice(&proxy.pixels);

    // Through a temporary and a rename, so a reader never meets a file that is
    // still being written. The same reason the document is saved that way.
    let temporary = file.with_extension("tpx-part");
    if std::fs::write(&temporary, &bytes).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return false;
    }
    if std::fs::rename(&temporary, file).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return false;
    }
    true
}

/// The size a proxy is made at for a picture drawn `longest_edge` across.
///
/// Rounded **up** to a power of two, so that resizing a frame by a point does
/// not throw the proxy away and build another. A dozen sizes per picture would
/// be a cache that never hits.
pub fn bucket(longest_edge: u32) -> u32 {
    const SMALLEST: u32 = 128;
    const LARGEST: u32 = 4096;
    let wanted = longest_edge.clamp(1, LARGEST);
    let mut at = SMALLEST;
    while at < wanted && at < LARGEST {
        at *= 2;
    }
    at
}

/// Bring the cache directory back under [`BUDGET`], oldest first.
///
/// By last-modified rather than by last-*read*: reading a file does not reliably
/// update its access time on any of the three platforms, and a cache that
/// evicted by a timestamp the system does not keep would evict at random.
/// Rebuilding a proxy that was still wanted costs one decode, so being
/// approximately right here is enough.
///
/// Returns how many bytes were removed.
pub fn evict(directory: &Path, budget: u64) -> u64 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };

    let mut held: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let Ok(data) = entry.metadata() else { continue };
        if !data.is_file() {
            continue;
        }
        let when = data.modified().unwrap_or(std::time::UNIX_EPOCH);
        total += data.len();
        held.push((when, data.len(), entry.path()));
    }

    if total <= budget {
        return 0;
    }

    held.sort_by_key(|(when, _, _)| *when);
    let mut freed = 0;
    for (_, size, path) in held {
        if total - freed <= budget {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            freed += size;
        }
    }
    freed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let at = std::env::temp_dir().join(format!("tessera-proxy-{name}"));
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at).expect("a scratch directory");
        at
    }

    fn a_proxy(width: u32, height: u32) -> Proxy {
        Proxy {
            width,
            height,
            pixels: vec![7; (width as usize) * (height as usize) * 4],
        }
    }

    #[test]
    fn a_proxy_written_is_a_proxy_read_back() {
        let dir = scratch("round-trip");
        let file = dir.join("one.tpx");
        let written = a_proxy(4, 3);

        assert!(write(&file, &written));
        let back = read(&file).expect("a proxy");
        assert_eq!((back.width, back.height), (4, 3));
        assert_eq!(back.pixels, written.pixels);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_proxy_is_a_cache_to_rebuild_rather_than_a_fault() {
        assert!(read(Path::new("nothing-of-this-name.tpx")).is_none());
    }

    #[test]
    fn a_truncated_proxy_is_thrown_away_rather_than_misread() {
        // The failure this guards is the worst kind: a half-written file that a
        // later read trusts and draws as garbage.
        let dir = scratch("truncated");
        let file = dir.join("short.tpx");
        write(&file, &a_proxy(8, 8));

        let mut bytes = std::fs::read(&file).expect("read");
        bytes.truncate(bytes.len() - 40);
        std::fs::write(&file, &bytes).expect("truncate");

        assert!(read(&file).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_that_is_not_a_proxy_at_all_is_thrown_away() {
        let dir = scratch("foreign");
        let file = dir.join("other.tpx");
        std::fs::write(&file, b"this is not a proxy, it is a note").expect("write");
        assert!(read(&file).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_proxy_that_disagrees_with_itself_is_not_written() {
        let dir = scratch("inconsistent");
        let file = dir.join("wrong.tpx");
        let wrong = Proxy {
            width: 10,
            height: 10,
            pixels: vec![0; 12],
        };
        assert!(!write(&file, &wrong));
        assert!(!file.exists(), "and nothing was left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_leaves_no_part_file_behind() {
        let dir = scratch("no-parts");
        let file = dir.join("clean.tpx");
        write(&file, &a_proxy(4, 4));

        let stray: Vec<_> = std::fs::read_dir(&dir)
            .expect("read")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with("-part"))
            .collect();
        assert!(stray.is_empty(), "left {stray:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_proxy_is_keyed_on_the_size_asked_for_as_well_as_the_file() {
        // Two frames showing one picture at two sizes want two proxies.
        let path = Path::new("/pictures/one.jpg");
        assert_ne!(name_for(path, Some(1), 256), name_for(path, Some(1), 512));
    }

    #[test]
    fn a_file_replaced_on_disk_gets_a_different_proxy() {
        // The reason the modification time is in the name, and the same reason
        // it is in the in-memory cache's key.
        let path = Path::new("/pictures/one.jpg");
        assert_ne!(name_for(path, Some(1), 256), name_for(path, Some(2), 256));
    }

    #[test]
    fn two_different_files_get_different_proxies() {
        assert_ne!(
            name_for(Path::new("/pictures/one.jpg"), Some(1), 256),
            name_for(Path::new("/pictures/two.jpg"), Some(1), 256)
        );
    }

    #[test]
    fn a_proxy_name_is_a_file_name_and_not_a_path() {
        // A path has separators and colons no filesystem agrees about, and a
        // deep one is longer than a file name may be.
        let name = name_for(
            Path::new("C:/a very/deep/path/with spaces/and-symbols/one.jpg"),
            Some(1),
            256,
        );
        assert!(!name.contains('/') && !name.contains('\\') && !name.contains(':'));
        assert!(name.len() < 64, "{name}");
    }

    #[test]
    fn nudging_a_frame_does_not_ask_for_a_different_proxy() {
        // Rounded up to a power of two, so resizing by a point does not throw
        // the proxy away and build another. A dozen sizes per picture would be a
        // cache that never hits.
        assert_eq!(bucket(300), bucket(301));
        assert_eq!(bucket(300), 512);
        assert_eq!(bucket(512), 512, "an exact power of two is its own bucket");
        assert_eq!(bucket(513), 1024);
    }

    #[test]
    fn a_tiny_picture_still_gets_a_sensible_proxy() {
        assert_eq!(bucket(1), 128);
        assert_eq!(bucket(0), 128, "and no size at all does not divide by zero");
    }

    #[test]
    fn a_huge_picture_is_capped() {
        // A proxy larger than any screen is a decode nobody benefits from.
        assert_eq!(bucket(100_000), 4096);
    }

    #[test]
    fn eviction_leaves_the_cache_under_its_budget() {
        let dir = scratch("evict");
        // Four proxies of 16x16 RGBA: 1024 bytes of pixels each, plus a header.
        for n in 0..4 {
            write(&dir.join(format!("{n}.tpx")), &a_proxy(16, 16));
            // Distinct modification times, so "oldest first" means something.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let total: u64 = std::fs::read_dir(&dir)
            .expect("read")
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        let budget = total / 2;

        let freed = evict(&dir, budget);
        assert!(freed > 0, "nothing was evicted");

        let after: u64 = std::fs::read_dir(&dir)
            .expect("read")
            .flatten()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        assert!(
            after <= budget,
            "{after} bytes left against a {budget} budget"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cache_under_its_budget_is_left_alone() {
        let dir = scratch("under");
        write(&dir.join("one.tpx"), &a_proxy(8, 8));
        assert_eq!(evict(&dir, BUDGET), 0);
        assert!(dir.join("one.tpx").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn evicting_a_directory_that_is_not_there_does_nothing() {
        assert_eq!(evict(Path::new("no-such-cache-directory"), 0), 0);
    }

    #[test]
    fn the_cache_lives_where_the_system_may_delete_it() {
        // Everything here can be rebuilt from the originals, so it belongs in
        // the cache directory rather than beside the preferences: asking the
        // system to back up derived data is wrong, and losing settings because
        // it cleared a cache is worse.
        let Some(at) = directory() else {
            return;
        };
        let shown = at.to_string_lossy().to_lowercase();
        assert!(
            shown.contains("cache") || shown.contains("local"),
            "proxies were put in {shown}"
        );
        assert!(shown.ends_with("proxies"));
    }
}
