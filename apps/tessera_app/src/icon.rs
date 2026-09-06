//! The application's window icon.
//!
//! Built from `assets/tessera-publisher-logotype.png`, which is **embedded**
//! rather than read from disk. A window icon loaded from a path beside the
//! executable is an icon that goes missing the moment the binary is copied
//! somewhere else, and the failure is silent.

/// How wide the icon is handed to the window manager.
///
/// The artwork is 1654 square, which is a sensible size for a master and a
/// wasteful one for a title bar: decoding it as-is means two and a half
/// million pixels, ten megabytes, held for the life of the process. 256 is the
/// largest size Windows asks for.
const SIZE: u32 = 256;

const ARTWORK: &[u8] = include_bytes!("../../../assets/tessera-publisher-logotype.png");

/// The icon, or `None` if the artwork could not be decoded.
///
/// `None` rather than a panic: an undecodable icon is a cosmetic fault, and
/// refusing to start a layout application over its title bar would be a
/// worse one.
pub fn load() -> Option<egui::IconData> {
    let decoded = image::load_from_memory(ARTWORK).ok()?;
    let scaled = decoded.resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3);
    let rgba = scaled.to_rgba8();

    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width: SIZE,
        height: SIZE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_decodes() {
        let icon = load().expect("the embedded artwork must decode");
        assert_eq!(icon.width, SIZE);
        assert_eq!(icon.height, SIZE);
        assert_eq!(
            icon.rgba.len(),
            (SIZE * SIZE * 4) as usize,
            "four bytes a pixel, which is what a window manager is handed"
        );
    }

    #[test]
    fn the_icon_is_not_blank() {
        // A file that decoded to nothing would pass every assertion above and
        // show an empty square.
        let icon = load().expect("decode");
        assert!(
            icon.rgba.chunks(4).any(|p| p[3] > 0),
            "something in it is opaque"
        );
    }

    #[test]
    fn the_mark_is_the_red_it_is_drawn_in() {
        // The logotype's field is #c3282d. Sampling the centre catches an
        // artwork swapped for the wrong file, which the two tests above would
        // not notice.
        let icon = load().expect("decode");
        let middle = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
        let pixel = &icon.rgba[middle..middle + 3];
        assert!(
            pixel[0] > pixel[1] + 40 && pixel[0] > pixel[2] + 40,
            "the centre of the mark is red, not {pixel:?}"
        );
    }
}
