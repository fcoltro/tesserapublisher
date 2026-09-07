//! Crop, bleed and registration marks, and the colour bar.
//!
//! Everything a guillotine operator and a press operator read, drawn outside the
//! trim where it will be cut away.
//!
//! ## Why registration marks are drawn in every ink at once
//!
//! A registration mark exists to show whether the plates line up. It must
//! therefore appear on **every** plate — which means 100% of every ink, not
//! black. A mark drawn in black appears on the black plate alone and shows
//! nothing about the other three, which is the one thing it was there to do.
//!
//! In an RGB export there are no plates, so a registration mark is meaningless;
//! it is drawn in black so the file still looks right, and the marks that matter
//! for an RGB proof are the crop marks.
//!
//! ## Why the marks are drawn last
//!
//! After the document, so nothing on the page can sit on top of a crop mark. A
//! photograph bleeding off the edge is drawn to the bleed and the marks are
//! beyond it, but an object dragged onto the pasteboard is not, and a crop mark
//! half covered by a stray rectangle is a crop mark somebody cuts to the wrong
//! place.

use pdf_writer::Content;
use tessera_color::Color;
use tessera_geometry::DocRect;
use tessera_layout::ResolvedPage;

use crate::ink::Ink;
use crate::options::{ExportOptions, MARK_LENGTH};

/// Draw whatever marks were asked for.
pub fn draw(content: &mut Content, page: &ResolvedPage, options: &ExportOptions, ink: &Ink) {
    let marks = &options.marks;
    if !marks.any() {
        return;
    }

    let trim = page.bounds;
    let bleed = page.bleed;

    content.save_state();
    // A hairline. Marks are cut through or trimmed away, so they exist to be
    // *seen* and aligned to, not to be a design element; heavier than this and
    // the mark itself has a width somebody has to aim at the middle of.
    content.set_line_width(0.25);

    if marks.crop {
        crop(content, trim, bleed, marks.offset, ink);
    }
    if marks.bleed {
        bleed_marks(content, trim, bleed, marks.offset, ink);
    }
    if marks.registration {
        registration(content, trim, marks.offset, ink);
    }
    if marks.colour_bar {
        colour_bar(content, trim, marks.offset, ink);
    }

    content.restore_state();
}

/// Where the guillotine goes: four pairs of lines, one pair per corner.
///
/// **Not crossing the corner.** A crop mark stops short of the trim so the cut
/// line is empty: a mark drawn through the corner would be visible on the
/// finished piece if the cut fell a hair outside it.
fn crop(content: &mut Content, trim: DocRect, bleed: DocRect, offset: f64, ink: &Ink) {
    ink.set_stroke(content, &registration_colour(ink));

    // The gap: never inside the bleed, because a mark over the ink is a mark
    // that cannot be seen.
    let gap = offset.max(bleed_gap(trim, bleed));

    let (l, r) = (trim.x, trim.x + trim.width);
    let (t, b) = (to_pdf(trim.y + trim.height), to_pdf(trim.y));

    for (x, dir) in [(l, -1.0), (r, 1.0)] {
        for y in [t, b] {
            line(content, x + dir * gap, y, x + dir * (gap + MARK_LENGTH), y);
        }
    }
    for (y, dir) in [(b, -1.0), (t, 1.0)] {
        for x in [l, r] {
            line(content, x, y + dir * gap, x, y + dir * (gap + MARK_LENGTH));
        }
    }
    content.stroke();
}

/// How far the ink is meant to run: a short mark at the bleed edge.
///
/// Beside the crop marks and shorter, which is the convention. Its whole job is
/// to let somebody check the artwork actually reaches the bleed rather than
/// stopping at the trim — the failure that leaves a white sliver down one side
/// of every copy.
fn bleed_marks(content: &mut Content, trim: DocRect, bleed: DocRect, offset: f64, ink: &Ink) {
    if bleed == trim {
        // No bleed to mark. Drawing one anyway would say the ink runs past the
        // trim when it does not.
        return;
    }
    ink.set_stroke(content, &registration_colour(ink));

    let gap = offset.max(bleed_gap(trim, bleed));
    let short = MARK_LENGTH / 2.0;
    let (l, r) = (bleed.x, bleed.x + bleed.width);
    let (t, b) = (to_pdf(bleed.y + bleed.height), to_pdf(bleed.y));

    for (x, dir) in [(l, -1.0), (r, 1.0)] {
        for y in [t, b] {
            line(content, x + dir * gap, y, x + dir * (gap + short), y);
        }
    }
    content.stroke();
}

/// Targets for checking the plates line up.
///
/// Drawn at the middle of each edge, in **every ink at once**, which is the only
/// thing that makes them work: a mark in black appears on the black plate alone
/// and says nothing about the other three.
fn registration(content: &mut Content, trim: DocRect, offset: f64, ink: &Ink) {
    const RADIUS: f64 = 5.0;
    ink.set_stroke(content, &registration_colour(ink));

    let middle_x = trim.x + trim.width / 2.0;
    let middle_y = to_pdf(trim.y + trim.height / 2.0);
    let out = offset + RADIUS + 2.0;

    let spots = [
        (middle_x, to_pdf(trim.y) - out),
        (middle_x, to_pdf(trim.y + trim.height) + out),
        (trim.x - out, middle_y),
        (trim.x + trim.width + out, middle_y),
    ];

    for (cx, cy) in spots {
        // A circle with a cross through it: the cross is what an operator lines
        // up, and the circle is what makes it findable at a glance.
        circle(content, cx, cy, RADIUS);
        line(content, cx - RADIUS * 1.6, cy, cx + RADIUS * 1.6, cy);
        line(content, cx, cy - RADIUS * 1.6, cx, cy + RADIUS * 1.6);
    }
    content.stroke();
}

/// Solid and tinted patches for checking density on press.
///
/// Only in a CMYK export. In RGB there are no plates to measure and no ink
/// densities to hold, so a colour bar would be a decorative strip of squares —
/// and a printer seeing one would reasonably assume the file was separated.
fn colour_bar(content: &mut Content, trim: DocRect, offset: f64, ink: &Ink) {
    if !ink.is_cmyk() {
        return;
    }

    const PATCH: f64 = 8.0;
    let y = to_pdf(trim.y + trim.height) + offset + 2.0;
    let mut x = trim.x;

    // Each ink solid, then each at a quarter, half and three-quarter tint. The
    // solids show density; the tints show dot gain, which is what actually
    // drifts during a run.
    for tint in [1.0, 0.75, 0.5, 0.25] {
        for plate in 0..4 {
            let mut values = [0.0f32; 4];
            values[plate] = tint;
            content.set_fill_cmyk(values[0], values[1], values[2], values[3]);
            content.rect(x as f32, y as f32, PATCH as f32, PATCH as f32);
            content.fill_nonzero();
            x += PATCH;
        }
    }
}

/// The colour a mark is drawn in.
///
/// 100% of every ink for a CMYK export, so the mark lands on every plate. Black
/// for RGB, where there are no plates and "every ink" means nothing.
fn registration_colour(ink: &Ink) -> Color {
    if ink.is_cmyk() {
        Color::Cmyk {
            c: 1.0,
            m: 1.0,
            y: 1.0,
            k: 1.0,
            a: 1.0,
        }
    } else {
        Color::BLACK
    }
}

/// How far the bleed reaches past the trim, at its widest.
///
/// Marks must clear it, or they are drawn over the artwork.
fn bleed_gap(trim: DocRect, bleed: DocRect) -> f64 {
    let left = trim.x - bleed.x;
    let top = trim.y - bleed.y;
    let right = (bleed.x + bleed.width) - (trim.x + trim.width);
    let bottom = (bleed.y + bleed.height) - (trim.y + trim.height);
    left.max(top).max(right).max(bottom).max(0.0)
}

/// Document y to PDF y, for a page whose origin is the trim corner.
///
/// The rest of the writer flips through `to_pdf_y`; marks are positioned
/// relative to the trim rather than to a rectangle, so they flip here. Both go
/// through one subtraction and neither invents a second convention.
fn to_pdf(y: f64) -> f64 {
    -y
}

fn line(content: &mut Content, x1: f64, y1: f64, x2: f64, y2: f64) {
    content.move_to(x1 as f32, y1 as f32);
    content.line_to(x2 as f32, y2 as f32);
}

/// Four Bézier arcs. PDF has no circle operator, and a mark drawn as a polygon
/// looks like a mark drawn as a polygon.
fn circle(content: &mut Content, cx: f64, cy: f64, r: f64) {
    const K: f64 = 0.552_284_749_8;
    let o = r * K;
    let (cx, cy, r, o) = (cx as f32, cy as f32, r as f32, o as f32);

    content.move_to(cx + r, cy);
    content.cubic_to(cx + r, cy + o, cx + o, cy + r, cx, cy + r);
    content.cubic_to(cx - o, cy + r, cx - r, cy + o, cx - r, cy);
    content.cubic_to(cx - r, cy - o, cx - o, cy - r, cx, cy - r);
    content.cubic_to(cx + o, cy - r, cx + r, cy - o, cx + r, cy);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trim() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 200.0,
        }
    }

    #[test]
    fn a_mark_clears_the_bleed_even_when_the_offset_is_smaller() {
        // A mark over the artwork is a mark that cannot be seen, and the bleed
        // is where the artwork reaches.
        let bleed = DocRect {
            x: -20.0,
            y: -20.0,
            width: 140.0,
            height: 240.0,
        };
        assert_eq!(bleed_gap(trim(), bleed), 20.0);
    }

    #[test]
    fn a_page_with_no_bleed_needs_no_clearance() {
        assert_eq!(bleed_gap(trim(), trim()), 0.0);
    }

    #[test]
    fn a_registration_mark_is_every_ink_at_once_in_cmyk() {
        // **The one thing that makes it work.** A mark in black appears on the
        // black plate alone and says nothing about whether the other three line
        // up, which is what it was there to answer.
        let conversion = tessera_color::managed::OutputProfile::screen()
            .expect("a profile")
            .ink_for_screen_colour(tessera_color::managed::Rendering::default())
            .expect("a conversion");
        let cmyk = Ink::Cmyk(Box::new(conversion));

        match registration_colour(&cmyk) {
            Color::Cmyk { c, m, y, k, .. } => {
                assert_eq!((c, m, y, k), (1.0, 1.0, 1.0, 1.0));
            }
            other => panic!("a registration mark was {other:?}"),
        }
    }

    #[test]
    fn a_registration_mark_is_black_in_rgb_where_there_are_no_plates() {
        assert_eq!(registration_colour(&Ink::Rgb), Color::BLACK);
    }

    fn a_page() -> tessera_layout::ResolvedPage {
        tessera_layout::ResolvedPage {
            bounds: trim(),
            margins: trim(),
            bleed: trim(),
            slug: trim(),
            columns: Vec::new(),
        }
    }

    #[test]
    fn nothing_is_drawn_when_nothing_was_asked_for() {
        let mut content = Content::new();
        draw(
            &mut content,
            &a_page(),
            &ExportOptions::default(),
            &Ink::Rgb,
        );
        assert!(content.finish().is_empty());
    }

    #[test]
    fn asking_for_marks_draws_some() {
        let mut content = Content::new();
        let options = ExportOptions {
            marks: crate::options::Marks::all(),
            ..Default::default()
        };
        draw(&mut content, &a_page(), &options, &Ink::Rgb);
        assert!(!content.finish().is_empty());
    }

    #[test]
    fn a_colour_bar_is_not_drawn_in_an_rgb_export() {
        // There are no plates to measure and no ink densities to hold, so a bar
        // would be a decorative strip of squares — and a printer seeing one
        // would reasonably assume the file was separated.
        let mut only_bar = Content::new();
        let options = ExportOptions {
            marks: crate::options::Marks {
                colour_bar: true,
                crop: false,
                bleed: false,
                registration: false,
                ..Default::default()
            },
            ..Default::default()
        };
        draw(&mut only_bar, &a_page(), &options, &Ink::Rgb);

        // The state save and line width still go out; no patches do.
        let written = String::from_utf8_lossy(&only_bar.finish()).into_owned();
        assert!(!written.contains(" k"), "a colour bar was drawn: {written}");
    }
}
