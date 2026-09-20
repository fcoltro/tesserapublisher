//! The application's window icon.
//!
//! Rendered by `build.rs` from `assets/tessera-publisher-logotype.png` and
//! **embedded** rather than read from disk. A window icon loaded from a path
//! beside the executable is an icon that goes missing the moment the binary
//! is copied somewhere else, and the failure is silent.

/// How wide the icon is handed to the window manager: the same number
/// `build.rs` scales to, and the largest size Windows asks for.
const SIZE: u32 = 256;

/// The icon's pixels, four bytes each, which is what a window manager is
/// handed. Scaled once at build time rather than at every start-up: the
/// artwork is one and three-quarter million pixels, and decoding it to
/// throw all but sixty-five thousand away was a tenth of a second on the
/// way to the first frame.
const RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/window-icon.rgba"));

// The two statements of the size agree, or this does not compile.
const _: () = assert!(RGBA.len() == (SIZE * SIZE * 4) as usize);

/// The icon, ready for `egui::ViewportBuilder::with_icon`.
pub fn load() -> egui::IconData {
    egui::IconData {
        rgba: RGBA.to_vec(),
        width: SIZE,
        height: SIZE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_not_blank() {
        // Pixels of the right count would pass the size assertion above and
        // show an empty square.
        let icon = load();
        assert!(
            icon.rgba.chunks(4).any(|p| p[3] > 0),
            "something in it is opaque"
        );
    }

    #[test]
    fn the_mark_is_the_red_it_is_drawn_in() {
        // The logotype's field is #dc1414. Sampling the centre catches an
        // artwork swapped for the wrong file, which the test above would not
        // notice.
        let icon = load();
        let middle = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
        let pixel = &icon.rgba[middle..middle + 3];
        assert!(
            pixel[0] > pixel[1] + 40 && pixel[0] > pixel[2] + 40,
            "the centre of the mark is red, not {pixel:?}"
        );
    }

    #[test]
    fn the_mark_is_padded_to_a_square_rather_than_stretched_to_one() {
        // The artwork is wider than it is tall, so a square made by padding
        // has clear rows above and below the mark, and one made by
        // stretching has none. The top-left pixel tells them apart.
        let icon = load();
        assert_eq!(icon.rgba[3], 0, "the corner is clear, not stretched into");
        // And the padding is symmetric: the first opaque row from the top
        // and from the bottom sit the same distance in.
        let row_is_clear = |y: u32| {
            let start = (y * SIZE * 4) as usize;
            icon.rgba[start..start + (SIZE * 4) as usize]
                .chunks(4)
                .all(|p| p[3] == 0)
        };
        let from_top = (0..SIZE).take_while(|&y| row_is_clear(y)).count();
        let from_bottom = (0..SIZE).rev().take_while(|&y| row_is_clear(y)).count();
        assert!(from_top > 0, "there is padding above");
        assert!(
            from_top.abs_diff(from_bottom) <= 1,
            "centred: {from_top} clear rows above, {from_bottom} below"
        );
    }
}
