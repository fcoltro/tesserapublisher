//! The icons: the one every platform shows in the title bar, and on Windows
//! the two the shell shows for the executable and for a `.tsrdf` document.
//!
//! All three are rendered here from the PNGs in `assets/`, at build time,
//! rather than committed beside them: a second copy of the artwork is one that
//! goes stale the day the first is re-exported, and nothing would say so.
//!
//! The window icon is written to `OUT_DIR` as raw pixels and included by
//! `src/icon.rs`. It used to be scaled at start-up instead, with its own
//! copy of the padding below — and when the mark was redrawn wider than it
//! is tall, that copy was the one that had none, and the title bar showed
//! the mark stretched to a square. One place pads now.
//!
//! On Windows the shell icons are embedded as resources — the application's
//! as ID 1, which is the one Explorer shows for the file, and the document's
//! as ID 2, which the installer names in `DefaultIcon`
//! (`apps/tessera_app/wix/main.wxs`). macOS and Linux take theirs from
//! `packaging/build.sh`.

use std::path::Path;

const LOGOTYPE: &str = "../../assets/tessera-publisher-logotype.png";
const DOCUMENT: &str = "../../assets/tessera-publisher-filetype-icon.png";

/// How wide the window icon is handed to the window manager. 256 is the
/// largest size Windows asks for. `src/icon.rs` states the same number and
/// proves the two agree against the length of what is written here.
const WINDOW_ICON_SIZE: u32 = 256;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={LOGOTYPE}");
    println!("cargo:rerun-if-changed={DOCUMENT}");

    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out);

    let logotype = square(LOGOTYPE);
    let scaled = image::imageops::resize(
        &logotype,
        WINDOW_ICON_SIZE,
        WINDOW_ICON_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    std::fs::write(out.join("window-icon.rgba"), scaled.into_raw()).expect("write the window icon");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    windows::embed(&logotype, &square(DOCUMENT), out);
}

/// The artwork, padded to a square and centred on transparency.
///
/// Neither PNG is square — the mark is wider than it is tall and the
/// document is a portrait page — and every consumer wants a square: an ICO
/// entry that is not one is drawn stretched to one, and so is a window icon.
///
/// **Refuses artwork with a transparent margin.** An export that includes
/// its artboard arrives with the mark sitting small in a clear field, and
/// every icon made from it is smaller by that margin at every size — on
/// all three platforms, since `packaging/build.sh` reads the same file and
/// has no tool to trim with. Failing the build is the one place that can
/// say so before an installer ships it.
fn square(png: &str) -> image::RgbaImage {
    let decoded = image::open(png)
        .unwrap_or_else(|e| panic!("{png}: {e}"))
        .to_rgba8();
    let (width, height) = decoded.dimensions();

    let opaque = |x: u32, y: u32| decoded.get_pixel(x, y)[3] > 0;
    let mut margin = Vec::new();
    if !(0..height).any(|y| opaque(0, y)) {
        margin.push("left");
    }
    if !(0..height).any(|y| opaque(width - 1, y)) {
        margin.push("right");
    }
    if !(0..width).any(|x| opaque(x, 0)) {
        margin.push("top");
    }
    if !(0..width).any(|x| opaque(x, height - 1)) {
        margin.push("bottom");
    }
    assert!(
        margin.is_empty(),
        "{png} has a transparent margin on the {} edge{}: crop it to the \
         artwork, or export it without the artboard",
        margin.join(", "),
        if margin.len() == 1 { "" } else { "s" },
    );

    let side = width.max(height);
    let mut square = image::RgbaImage::new(side, side);
    let x = (side - width) / 2;
    let y = (side - height) / 2;
    image::imageops::overlay(&mut square, &decoded, i64::from(x), i64::from(y));
    square
}

#[cfg(not(target_os = "windows"))]
mod windows {
    pub fn embed(_: &image::RgbaImage, _: &image::RgbaImage, _: &std::path::Path) {}
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;

    /// The sizes the shell asks for, from the list view to the "extra large"
    /// tiles. 256 is the ICO ceiling, and the largest Windows requests.
    const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

    pub fn embed(application: &image::RgbaImage, document: &image::RgbaImage, out: &Path) {
        let app_ico = out.join("application.ico");
        let document_ico = out.join("document.ico");
        write_ico(application, &app_ico);
        write_ico(document, &document_ico);

        winresource::WindowsResource::new()
            .set_icon_with_id(app_ico.to_str().expect("utf-8 path"), "1")
            .set_icon_with_id(document_ico.to_str().expect("utf-8 path"), "2")
            .compile()
            .expect("embed the Windows icon resources");
    }

    /// One `.ico` holding the square artwork at every size in `SIZES`.
    fn write_ico(square: &image::RgbaImage, ico: &Path) {
        let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
        for size in SIZES {
            let scaled =
                image::imageops::resize(square, size, size, image::imageops::FilterType::Lanczos3);
            let entry = ico::IconImage::from_rgba_data(size, size, scaled.into_raw());
            // The layout every reader handles: bitmaps up to 128, and PNG only
            // for the 256, where Windows itself expects it. Left to choose, the
            // crate would compress every anti-aliased entry as PNG.
            let encoded = if size == 256 {
                ico::IconDirEntry::encode_as_png(&entry)
            } else {
                ico::IconDirEntry::encode_as_bmp(&entry)
            };
            dir.add_entry(encoded.expect("encode an icon entry"));
        }
        let file = std::fs::File::create(ico).unwrap_or_else(|e| panic!("{ico:?}: {e}"));
        dir.write(std::io::BufWriter::new(file))
            .unwrap_or_else(|e| panic!("{ico:?}: {e}"));
    }
}
