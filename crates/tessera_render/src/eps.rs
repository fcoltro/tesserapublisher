//! Encapsulated PostScript: artwork as a program, placed.
//!
//! An EPS is a PostScript program that draws one picture, and PostScript is
//! a language: showing one faithfully means running it. Rust has no
//! PostScript interpreter, so this reads EPS the two ways that need none of
//! its own:
//!
//! - **Ghostscript.** The file is converted once to a PDF, cropped to its
//!   bounding box, kept in a cache beside the image proxies, and from then on
//!   it is a placed PDF: drawn sharp on screen and copied as vectors into an
//!   export. The Windows installer ships a copy beside the application,
//!   which is looked for first; elsewhere it is found where it is installed.
//!   It stays a separate program, run and not linked: it is AGPL, which
//!   Tessera's GPL-3.0 allows beside it, with its licence and the way to its
//!   source shipped too.
//! - **Its preview, otherwise.** Most EPS files carry a picture of
//!   themselves for programs that cannot run them: a TIFF in a binary
//!   ("DOS") EPS, or hex lines in an EPSI. That picture is what shows and
//!   what prints, as it did in every layout program before PDF.
//!
//! Its size is its `%%BoundingBox`, in points, which needs neither.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Whether this path is an EPS, by its extension.
pub fn is_eps(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["eps", "epsf", "epsi"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}

/// The PostScript part of an EPS file and, for a binary one, its TIFF
/// preview. A binary EPS begins `C5 D0 D3 C6` and says where each part is.
fn parts(bytes: &[u8]) -> (&[u8], Option<&[u8]>) {
    if bytes.len() >= 30 && bytes[..4] == [0xC5, 0xD0, 0xD3, 0xC6] {
        let word = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
        };
        let slice = |start: usize, len: usize| bytes.get(start..start.checked_add(len)?);
        let postscript = slice(word(4), word(8)).unwrap_or(&[]);
        let tiff = slice(word(20), word(24)).filter(|t| !t.is_empty());
        return (postscript, tiff);
    }
    (bytes, None)
}

/// The bounding box the file states — `%%HiResBoundingBox` where it gives
/// one, `%%BoundingBox` else — as left, bottom, right and top in points.
pub fn bounding_box(bytes: &[u8]) -> Option<(f64, f64, f64, f64)> {
    let (postscript, _) = parts(bytes);
    // The comments are at the head, or at the end after `(atend)`.
    let text = String::from_utf8_lossy(postscript);
    let read = |key: &str| {
        text.lines()
            .filter_map(|l| l.strip_prefix(key))
            .filter_map(|rest| {
                let n: Vec<f64> = rest
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                (n.len() == 4).then(|| (n[0], n[1], n[2], n[3]))
            })
            .next()
    };
    read("%%HiResBoundingBox:")
        .or_else(|| read("%%BoundingBox:"))
        .filter(|(l, b, r, t)| r > l && t > b)
}

/// The size an EPS asks to be, in points, from its bounding box.
pub fn size(path: &Path) -> Option<(f64, f64)> {
    let bytes = std::fs::read(path).ok()?;
    let (l, b, r, t) = bounding_box(&bytes)?;
    Some((r - l, t - b))
}

/// The preview an EPS carries, as straight RGBA: a binary EPS's TIFF, or
/// an EPSI's hex bitmap. `None` for a file that carries neither.
pub fn preview(path: &Path) -> Option<(Vec<u8>, (u32, u32))> {
    let bytes = std::fs::read(path).ok()?;
    preview_of(&bytes)
}

fn preview_of(bytes: &[u8]) -> Option<(Vec<u8>, (u32, u32))> {
    let (postscript, tiff) = parts(bytes);
    if let Some(tiff) = tiff {
        let decoded = image::load_from_memory_with_format(tiff, image::ImageFormat::Tiff)
            .ok()?
            .to_rgba8();
        let (w, h) = decoded.dimensions();
        return Some((decoded.into_raw(), (w, h)));
    }
    epsi(&String::from_utf8_lossy(postscript))
}

/// An EPSI preview: `%%BeginPreview: width height depth lines`, then the
/// rows as hex after a `%`, 1 or 8 bits a pixel, where 0 is white for one
/// bit and black for eight — as the format says.
fn epsi(text: &str) -> Option<(Vec<u8>, (u32, u32))> {
    let mut lines = text.lines();
    let header = lines.find_map(|l| l.strip_prefix("%%BeginPreview:"))?;
    let n: Vec<u32> = header
        .split_whitespace()
        .filter_map(|v| v.parse().ok())
        .collect();
    let (width, height, depth) = (*n.first()?, *n.get(1)?, *n.get(2)?);
    if width == 0 || height == 0 || !matches!(depth, 1 | 8) {
        return None;
    }
    let mut data = Vec::new();
    for line in lines {
        if line.starts_with("%%EndPreview") {
            break;
        }
        let hex = line.trim_start_matches('%').trim();
        let digits: Vec<u8> = hex
            .bytes()
            .filter_map(|b| (b as char).to_digit(16).map(|d| d as u8))
            .collect();
        data.extend(digits.chunks_exact(2).map(|p| p[0] << 4 | p[1]));
    }
    let row = (width * depth).div_ceil(8) as usize;
    if data.len() < row * height as usize {
        return None;
    }
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let grey = if depth == 1 {
                let bit = data[y * row + x / 8] & (0x80 >> (x % 8)) != 0;
                if bit { 0 } else { 255 }
            } else {
                255 - data[y * row + x]
            };
            rgba.extend_from_slice(&[grey, grey, grey, 255]);
        }
    }
    Some((rgba, (width, height)))
}

/// Ghostscript's command-line program, if the machine has one: on the
/// search path, or where its Windows installer puts it. Looked for once.
pub fn ghostscript() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let names: &[&str] = if cfg!(windows) {
                &["gswin64c.exe", "gswin32c.exe", "gs.exe"]
            } else {
                &["gs"]
            };
            // The copy an installer put beside the application — in a Mac
            // bundle, among its resources — before any other, so the version
            // shipped is the version used.
            let bundled = std::env::current_exe().ok().and_then(|exe| {
                let beside = exe.parent()?.to_path_buf();
                [
                    beside.join("ghostscript"),
                    beside.join("..").join("Resources").join("ghostscript"),
                ]
                .into_iter()
                .flat_map(|dir| names.iter().map(move |n| dir.join("bin").join(n)))
                .find(|p| p.is_file())
            });
            if bundled.is_some() {
                return bundled;
            }
            let on_path = std::env::var_os("PATH").and_then(|paths| {
                std::env::split_paths(&paths)
                    .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
                    .find(|p| p.is_file())
            });
            on_path.or_else(|| {
                // C:\Program Files\gs\gs10.03.1\bin\gswin64c.exe, the newest.
                let root = PathBuf::from(std::env::var_os("ProgramFiles")?).join("gs");
                let mut versions: Vec<PathBuf> = std::fs::read_dir(root)
                    .ok()?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .collect();
                versions.sort();
                versions
                    .into_iter()
                    .rev()
                    .flat_map(|v| names.iter().map(move |n| v.join("bin").join(n)))
                    .find(|p| p.is_file())
            })
        })
        .as_deref()
}

/// Where an EPS's PDF is kept: a name made from its path and when it was
/// last changed, so an edited file is converted again.
fn cached_pdf(path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    modified.hash(&mut hasher);
    let folder = crate::proxies::directory()
        .unwrap_or_else(std::env::temp_dir)
        .join("eps");
    Some(folder.join(format!("{:016x}.pdf", hasher.finish())))
}

/// How long Ghostscript is given for one file: long enough for a heavy
/// map, short enough that a program that never ends does not hang the
/// application.
const GIVE_UP: std::time::Duration = std::time::Duration::from_secs(60);

/// The EPS as a PDF, converted by Ghostscript the first time it is asked
/// for and kept; `None` without Ghostscript, or when it could not.
pub fn as_pdf(path: &Path) -> Option<PathBuf> {
    let out = cached_pdf(path)?;
    if out.is_file() {
        return Some(out);
    }
    let gs = ghostscript()?;
    std::fs::create_dir_all(out.parent()?).ok()?;
    let partial = out.with_extension("part");
    let mut child = std::process::Command::new(gs)
        .args([
            "-q",
            "-dSAFER",
            "-dBATCH",
            "-dNOPAUSE",
            "-dEPSCrop",
            "-sDEVICE=pdfwrite",
        ])
        .arg(format!("-sOutputFile={}", partial.display()))
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if started.elapsed() < GIVE_UP => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    if !finished || !partial.is_file() {
        let _ = std::fs::remove_file(&partial);
        return None;
    }
    // Renamed into place whole, so a half-written PDF is never read.
    std::fs::rename(&partial, &out).ok()?;
    Some(out)
}

/// The file to read for this artwork: an EPS's PDF when Ghostscript has
/// made or can make one, the path itself otherwise.
pub fn effective(path: &Path) -> PathBuf {
    if is_eps(path)
        && let Some(pdf) = as_pdf(path)
    {
        return pdf;
    }
    path.to_path_buf()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const PLAIN: &str = "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 50\n%%HiResBoundingBox: 0 0 100.5 50.25\n%%EndComments\n0 0 moveto 100 50 lineto stroke\n";

    #[test]
    fn the_size_is_the_bounding_box() {
        assert_eq!(
            bounding_box(PLAIN.as_bytes()),
            Some((0.0, 0.0, 100.5, 50.25))
        );
        let low = "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 10 20 110 70\n";
        assert_eq!(
            bounding_box(low.as_bytes()),
            Some((10.0, 20.0, 110.0, 70.0))
        );
        assert_eq!(bounding_box(b"%!PS\nno box here\n"), None);
        assert!(is_eps(Path::new("logo.EPS")) && !is_eps(Path::new("logo.ps")));
    }

    /// A binary EPS whose preview is a TIFF of `rgba`.
    pub(crate) fn binary(postscript: &str, width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut tiff = Vec::new();
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(width, height, rgba.to_vec()).unwrap(),
        )
        .write_to(
            &mut std::io::Cursor::new(&mut tiff),
            image::ImageFormat::Tiff,
        )
        .unwrap();
        let ps = postscript.as_bytes();
        let mut out = vec![0xC5, 0xD0, 0xD3, 0xC6];
        let ps_at = 30u32;
        let tiff_at = ps_at + ps.len() as u32;
        for word in [ps_at, ps.len() as u32, 0, 0, tiff_at, tiff.len() as u32] {
            out.extend(word.to_le_bytes());
        }
        out.extend([0xFF, 0xFF]); // no checksum
        out.extend(ps);
        out.extend(tiff);
        out
    }

    #[test]
    fn a_binary_eps_shows_its_tiff_preview_and_keeps_its_box() {
        let red: Vec<u8> = [255, 0, 0, 255].repeat(4 * 2);
        let bytes = binary(PLAIN, 4, 2, &red);
        assert_eq!(bounding_box(&bytes), Some((0.0, 0.0, 100.5, 50.25)));
        let (rgba, size) = preview_of(&bytes).expect("a preview");
        assert_eq!(size, (4, 2));
        assert_eq!(&rgba[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn an_epsi_preview_is_read_from_its_hex_lines() {
        // Two rows of eight one-bit pixels: black on the left half.
        let text = format!("{PLAIN}%%BeginPreview: 8 2 1 2\n% F0\n% F0\n%%EndPreview\n");
        let (rgba, size) = preview_of(text.as_bytes()).expect("a preview");
        assert_eq!(size, (8, 2));
        assert_eq!(&rgba[..4], &[0, 0, 0, 255], "a set bit is black");
        assert_eq!(
            &rgba[4 * 4..4 * 5],
            &[255, 255, 255, 255],
            "a clear one white"
        );
        assert!(
            preview_of(PLAIN.as_bytes()).is_none(),
            "no preview, none shown"
        );
    }

    #[test]
    fn with_ghostscript_an_eps_becomes_a_pdf_of_its_size() {
        // Only where Ghostscript is installed; its absence is not a fault.
        if ghostscript().is_none() {
            return;
        }
        let path = std::env::temp_dir().join(format!("tessera-eps-{}.eps", std::process::id()));
        std::fs::write(&path, PLAIN).unwrap();
        let pdf = as_pdf(&path).expect("converted");
        let (w, h) = crate::images::pdf_size(&pdf).expect("a PDF");
        assert!(
            (w - 100.5).abs() < 1.5 && (h - 50.25).abs() < 1.5,
            "{w} x {h}"
        );
        let _ = std::fs::remove_file(path);
    }
}
