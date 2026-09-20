//! The icons Windows shows for the executable and for a `.tsrdf` document.
//!
//! Both are rendered here from the PNGs in `assets/`, at build time, rather
//! than committed as `.ico` files beside them: a second copy of the artwork is
//! one that goes stale the day the first is re-exported, and nothing would
//! say so. They are embedded as icon resources — the application's as ID 1,
//! which is the one Explorer shows for the file, and the document's as ID 2,
//! which the installer names in `DefaultIcon` (`apps/tessera_app/wix/main.wxs`).
//!
//! On any other platform this does nothing; macOS and Linux take their icons
//! from `packaging/build.sh`.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/tessera-publisher-logotype.png");
    println!("cargo:rerun-if-changed=../../assets/tessera-publisher-filetype-icon.png");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    windows::embed();
}

#[cfg(not(target_os = "windows"))]
mod windows {
    pub fn embed() {}
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;

    /// The sizes the shell asks for, from the list view to the "extra large"
    /// tiles. 256 is the ICO ceiling, and the largest Windows requests.
    const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

    pub fn embed() {
        let out = std::env::var("OUT_DIR").expect("OUT_DIR");
        let out = Path::new(&out);
        let app = out.join("application.ico");
        let document = out.join("document.ico");
        write_ico("../../assets/tessera-publisher-logotype.png", &app);
        write_ico(
            "../../assets/tessera-publisher-filetype-icon.png",
            &document,
        );

        winresource::WindowsResource::new()
            .set_icon_with_id(app.to_str().expect("utf-8 path"), "1")
            .set_icon_with_id(document.to_str().expect("utf-8 path"), "2")
            .compile()
            .expect("embed the Windows icon resources");
    }

    /// One `.ico` holding the artwork at every size in `SIZES`.
    ///
    /// The artwork is padded to a square first, centred on transparency: the
    /// document icon is a portrait page, and an ICO entry that is not square
    /// is drawn stretched to one.
    fn write_ico(png: &str, ico: &Path) {
        let decoded = image::open(png)
            .unwrap_or_else(|e| panic!("{png}: {e}"))
            .to_rgba8();
        let side = decoded.width().max(decoded.height());
        let mut square = image::RgbaImage::new(side, side);
        let x = (side - decoded.width()) / 2;
        let y = (side - decoded.height()) / 2;
        image::imageops::overlay(&mut square, &decoded, i64::from(x), i64::from(y));

        let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
        for size in SIZES {
            let scaled =
                image::imageops::resize(&square, size, size, image::imageops::FilterType::Lanczos3);
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
