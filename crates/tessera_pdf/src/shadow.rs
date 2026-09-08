//! Drop shadows, as the luminosity soft mask a PDF can actually carry.
//!
//! There is no blur operator in PDF, and a gaussian of a rectangle is not any
//! gradient PDF can express. What a shadow *is*, in a PDF, is a solid rectangle
//! of the shadow's colour with a greyscale image masking it — white where the
//! shadow is solid, black where it has faded out. That image is built here, on
//! the CPU, with no rasteriser and no GPU: this crate must not reach for either.
//!
//! ## Why it was written as nothing until now
//!
//! Because the writer could not embed an image at all. Writing a *hard* offset
//! duplicate instead would have been worse than writing nothing — a missing
//! shadow is obviously missing, and a hard one looks like somebody meant it.
//!
//! ## Three box blurs, not a gaussian
//!
//! A true gaussian over a large radius is expensive and, at the sizes a mask
//! needs, indistinguishable from three passes of a box blur — which is the
//! standard result and is what every compositor does. Each pass is a running sum,
//! so the cost is the same whatever the radius: a 144-point blur costs what a
//! 2-point one does.

use tessera_document::shadow::Shadow;

/// How many samples per point the mask is built at.
///
/// **Not the document's resolution and not the artwork's.** A shadow is a soft
/// edge with no detail in it, so it carries no information a fine grid would
/// preserve — and a mask at 300ppi for a full-page frame is a nine-megapixel
/// greyscale image embedded to describe a gradient. Two per point is past the
/// point where the banding is visible.
const SAMPLES_PER_POINT: f64 = 2.0;

/// The widest a mask may be, in samples.
///
/// A spread-wide frame at two samples a point is about 2400 across, so this is
/// not a limit real work reaches. It is here because the alternative to a bound
/// is a document that allocates gigabytes for one shadow.
const MOST_SAMPLES: usize = 4096;

/// A shadow's mask: a greyscale image, and where it sits.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    /// One byte per sample. 255 is fully shadowed, 0 is clear.
    pub coverage: Vec<u8>,
}

/// Build the mask for a shadow cast by a rectangle of `w` × `h` points.
///
/// The mask is bigger than the rectangle, by the blur's reach on every side —
/// a shadow that stopped at the shape's edge would have a hard edge, which is
/// the one thing it must not have.
pub fn mask(shadow: &Shadow, w: f64, h: f64) -> Option<Mask> {
    let sigma = shadow.std_dev();
    // The visible extent of a gaussian, past which the coverage is under half a
    // step of an eight-bit channel and cannot be drawn anyway.
    let bleed = (sigma * 3.0).ceil();

    let width = samples(w + bleed * 2.0)?;
    let height = samples(h + bleed * 2.0)?;

    // The rectangle, solid, inset by the bleed it will spread into.
    let inset = (bleed * SAMPLES_PER_POINT).round() as usize;
    let mut coverage = vec![0u8; width * height];
    for y in inset..height.saturating_sub(inset) {
        for x in inset..width.saturating_sub(inset) {
            coverage[y * width + x] = 255;
        }
    }

    if sigma > 0.0 {
        // Three passes approximate a gaussian; the radius that makes three
        // boxes match a given sigma is the standard one.
        let radius = ((sigma * SAMPLES_PER_POINT) * 0.939_3).round() as usize;
        if radius > 0 {
            for _ in 0..3 {
                coverage = blur_rows(&coverage, width, height, radius);
                coverage = transpose(&coverage, width, height);
                coverage = blur_rows(&coverage, height, width, radius);
                coverage = transpose(&coverage, height, width);
            }
        }
    }

    Some(Mask {
        width,
        height,
        coverage,
    })
}

/// How many points of margin the mask adds on each side.
///
/// The caller needs this to place the mask: it is drawn bigger than the shape it
/// belongs to, and centred on it.
pub fn bleed(shadow: &Shadow) -> f64 {
    (shadow.std_dev() * 3.0).ceil()
}

fn samples(points: f64) -> Option<usize> {
    let count = (points * SAMPLES_PER_POINT).ceil();
    if !count.is_finite() || count < 1.0 {
        return None;
    }
    Some((count as usize).min(MOST_SAMPLES))
}

/// One horizontal box blur pass, by running sum.
///
/// The running sum is what makes the radius free: a 144-point blur costs the
/// same as a 2-point one, because each output sample is the previous one plus
/// what entered the window minus what left it.
fn blur_rows(source: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    let mut out = vec![0u8; source.len()];
    let span = radius * 2 + 1;

    for y in 0..height {
        let row = y * width;
        // Edges are clamped rather than treated as zero. Treating what is
        // outside as clear would darken nothing but would lighten the shadow's
        // own edges, which is a visible ring.
        let at = |x: isize| -> u32 {
            let x = x.clamp(0, width as isize - 1) as usize;
            u32::from(source[row + x])
        };

        let mut sum: u32 = (-(radius as isize)..=(radius as isize)).map(at).sum();
        for x in 0..width {
            out[row + x] = (sum / span as u32) as u8;
            sum = sum + at(x as isize + radius as isize + 1) - at(x as isize - radius as isize);
        }
    }
    out
}

/// Rows into columns, so the second pass of a separable blur is the first again.
///
/// Two passes over one axis is not a blur. Transposing rather than writing a
/// vertical pass keeps one blur function, and one function is one place for the
/// off-by-one to be wrong in.
fn transpose(source: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; source.len()];
    for y in 0..height {
        for x in 0..width {
            out[x * height + y] = source[y * width + x];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shadow(blur: f64) -> Shadow {
        Shadow {
            blur,
            ..Shadow::TYPICAL
        }
    }

    #[test]
    fn a_hard_shadow_is_solid_to_its_edge() {
        // Zero blur is a real thing to want rather than a degenerate case, and
        // it must not come out soft.
        let made = mask(&shadow(0.0), 20.0, 10.0).expect("mask");
        assert!(made.coverage.contains(&255));
        assert!(
            made.coverage.iter().all(|c| *c == 0 || *c == 255),
            "a hard shadow came out with soft edges"
        );
    }

    #[test]
    fn a_blurred_shadow_fades_rather_than_stopping() {
        // The one thing a shadow must not have is a hard edge. Values strictly
        // between clear and solid are the whole of what makes it a shadow.
        let made = mask(&shadow(6.0), 40.0, 20.0).expect("mask");
        assert!(
            made.coverage.iter().any(|c| *c > 0 && *c < 255),
            "the blur produced no partial coverage"
        );
    }

    #[test]
    fn the_mask_is_bigger_than_the_shape_by_the_blur() {
        // A mask that stopped at the shape's edge would clip the shadow into a
        // hard edge, which is exactly the thing being avoided.
        let soft = shadow(8.0);
        let made = mask(&soft, 40.0, 20.0).expect("mask");
        let margin = bleed(&soft);
        assert!(margin > 0.0);
        assert!(
            made.width as f64 > 40.0 * SAMPLES_PER_POINT,
            "the mask is no wider than the shape"
        );
    }

    #[test]
    fn the_middle_stays_solid_however_soft_the_edge() {
        // A blur that dimmed the centre would make the whole shadow paler than
        // it was asked to be, which reads as the opacity being wrong.
        let made = mask(&shadow(4.0), 120.0, 120.0).expect("mask");
        let middle = made.coverage[made.height / 2 * made.width + made.width / 2];
        assert!(middle > 250, "the centre faded to {middle}");
    }

    #[test]
    fn the_cost_does_not_grow_with_the_radius() {
        // The running sum is what makes this true, and it is the reason a
        // 144-point blur is affordable at all. Asserted as a shape check: the
        // mask for a huge blur is still built, and still bounded.
        let made = mask(&shadow(tessera_document::shadow::MOST_BLUR), 50.0, 50.0).expect("mask");
        assert!(made.width <= MOST_SAMPLES && made.height <= MOST_SAMPLES);
        assert_eq!(made.coverage.len(), made.width * made.height);
    }

    #[test]
    fn a_shape_with_no_size_makes_no_mask() {
        // Rather than a zero-by-zero image, which is a valid allocation and an
        // invalid PDF object.
        assert!(mask(&shadow(0.0), 0.0, 0.0).is_none());
    }

    #[test]
    fn transposing_twice_is_the_identity() {
        // The separable blur runs the same pass over both axes by transposing
        // between them, so a wrong transpose is a blur that is only horizontal
        // and nothing would say so.
        let source: Vec<u8> = (0..12).collect();
        let once = transpose(&source, 4, 3);
        let back = transpose(&once, 3, 4);
        assert_eq!(back, source);
    }
}
