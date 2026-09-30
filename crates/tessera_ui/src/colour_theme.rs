//! A colour theme taken from what is on the page: InDesign's Color Theme
//! tool.
//!
//! From a picture, the five colours that most of it is made of; from a drawn
//! object or a text frame, the colours it is actually painted in. Either way
//! a handful of swatches somebody can build the rest of the page from.
//!
//! **k-means, seeded deterministically.** The seeds are the most populous
//! cells of a coarse histogram, taken greedily so no two are close — which is
//! what makes the result the *different* colours of the picture rather than
//! five shades of its sky, and makes the same picture give the same theme
//! every time it is clicked. Pure arithmetic over RGBA bytes, so it is tested
//! without a file.

use tessera_color::Color;

/// A theme picked up, and what it was picked up from.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    /// A name to give its swatches: the picture's file name, or the kind of
    /// object it came from.
    pub name: String,
    pub colours: Vec<Color>,
}

/// How many colours a theme holds, as InDesign's does.
pub const SIZE: usize = 5;

/// How far apart two seeds must be, as a distance in 0–1 RGB.
const APART: f32 = 0.18;

/// How many pixels are looked at: every `n`th, so a large picture costs what
/// a small one does.
const SAMPLES: usize = 20_000;

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The theme of a picture, from its pixels as RGBA bytes: up to [`SIZE`]
/// colours, the most of the picture first. Transparent pixels count for
/// nothing; a picture that is all one colour gives one.
pub fn from_pixels(rgba: &[u8]) -> Vec<[f32; 3]> {
    let step = (rgba.len() / 4 / SAMPLES).max(1);
    let points: Vec<[f32; 3]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .step_by(step)
        .filter(|p| p[3] >= 128)
        .map(|p| {
            [
                f32::from(p[0]) / 255.0,
                f32::from(p[1]) / 255.0,
                f32::from(p[2]) / 255.0,
            ]
        })
        .collect();
    if points.is_empty() {
        return Vec::new();
    }

    // A 16-level histogram per channel, and the mean of each cell.
    let cell = |p: &[f32; 3]| {
        let q = |v: f32| ((v * 15.999) as usize).min(15);
        q(p[0]) * 256 + q(p[1]) * 16 + q(p[2])
    };
    let mut sums = vec![([0.0f32; 3], 0usize); 4096];
    for p in &points {
        let c = &mut sums[cell(p)];
        for (total, v) in c.0.iter_mut().zip(p) {
            *total += v;
        }
        c.1 += 1;
    }
    let mut cells: Vec<([f32; 3], usize)> = sums
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(s, n)| ([s[0] / n as f32, s[1] / n as f32, s[2] / n as f32], n))
        .collect();
    cells.sort_by(|a, b| b.1.cmp(&a.1).then(a.0[0].total_cmp(&b.0[0])));

    let mut seeds: Vec<[f32; 3]> = Vec::new();
    for (colour, _) in &cells {
        if seeds.iter().all(|s| distance(*s, *colour) >= APART) {
            seeds.push(*colour);
            if seeds.len() == SIZE {
                break;
            }
        }
    }

    // A few rounds of k-means from those seeds, so each colour is the middle
    // of what it stands for rather than a histogram cell's corner.
    let mut centres = seeds;
    let mut counts = vec![0usize; centres.len()];
    for _ in 0..8 {
        let mut sums = vec![[0.0f32; 3]; centres.len()];
        counts = vec![0usize; centres.len()];
        for p in &points {
            let nearest = centres
                .iter()
                .enumerate()
                .map(|(i, c)| (i, distance(*c, *p)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(0, |(i, _)| i);
            for (total, v) in sums[nearest].iter_mut().zip(p) {
                *total += v;
            }
            counts[nearest] += 1;
        }
        for (i, c) in centres.iter_mut().enumerate() {
            if counts[i] > 0 {
                let n = counts[i] as f32;
                *c = [sums[i][0] / n, sums[i][1] / n, sums[i][2] / n];
            }
        }
    }
    let mut ranked: Vec<([f32; 3], usize)> = centres.into_iter().zip(counts).collect();
    ranked.sort_by_key(|r| std::cmp::Reverse(r.1));
    ranked
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(c, _)| c)
        .collect()
}

/// The theme of a drawn object: the colours it is painted in, in the order
/// given, each once, up to [`SIZE`]. Colours with no alpha are nothing to
/// build a page from and are left out.
pub fn from_colours(colours: &[Color]) -> Vec<Color> {
    let mut out: Vec<Color> = Vec::new();
    for colour in colours {
        let rgba = colour.to_rgb_f32();
        if rgba[3] <= 0.0 {
            continue;
        }
        let seen = out.iter().any(|c| {
            let o = c.to_rgb_f32();
            distance([o[0], o[1], o[2]], [rgba[0], rgba[1], rgba[2]]) < 1e-3
        });
        if !seen {
            out.push(colour.clone());
            if out.len() == SIZE {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(parts: &[([u8; 3], usize)]) -> Vec<u8> {
        parts
            .iter()
            .flat_map(|(c, n)| std::iter::repeat_n([c[0], c[1], c[2], 255], *n))
            .flatten()
            .collect()
    }

    #[test]
    fn the_colours_a_picture_is_made_of_come_out_the_most_first() {
        let picture = pixels(&[
            ([200, 30, 30], 600),
            ([20, 40, 200], 300),
            ([240, 240, 240], 100),
        ]);
        let theme = from_pixels(&picture);
        assert_eq!(theme.len(), 3);
        assert!(
            theme[0][0] > 0.7 && theme[0][2] < 0.2,
            "red first: {theme:?}"
        );
        assert!(theme[1][2] > 0.7, "then blue: {theme:?}");
    }

    #[test]
    fn five_shades_of_one_sky_are_not_five_colours() {
        // Close neighbours are one colour; the different one is kept.
        let picture = pixels(&[
            ([100, 150, 220], 200),
            ([104, 152, 222], 200),
            ([98, 148, 218], 200),
            ([250, 200, 40], 100),
        ]);
        let theme = from_pixels(&picture);
        assert_eq!(theme.len(), 2, "{theme:?}");
    }

    #[test]
    fn the_same_picture_gives_the_same_theme() {
        let picture = pixels(&[([10, 200, 90], 500), ([200, 10, 150], 500)]);
        assert_eq!(from_pixels(&picture), from_pixels(&picture));
    }

    #[test]
    fn a_transparent_picture_has_no_theme() {
        assert!(from_pixels(&[0, 0, 0, 0, 255, 0, 0, 0]).is_empty());
    }

    #[test]
    fn an_object_s_theme_is_its_own_colours_once_each() {
        let red = Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let clear = Color::Rgb {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        let theme = from_colours(&[red.clone(), clear, Color::BLACK, red]);
        assert_eq!(
            theme,
            vec![
                Color::Rgb {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                Color::BLACK
            ]
        );
    }
}
