//! Spot inks, as the plates they are.
//!
//! A spot colour is not a mixture of process inks: it is a pot of a specific
//! ink, and a job that names one is a job with an extra plate on the press. It
//! was being written as its process fallback — which prints something about the
//! right colour, on the wrong plates, and silently turns a two-colour job into a
//! four-colour one.
//!
//! ## What PDF calls it
//!
//! A `/Separation` colour space: a name, an alternate space to *approximate* it
//! in when nobody can print the real ink, and a **tint transform** — a function
//! from one number, how much ink, to a colour in that alternate space. Setting a
//! colour is then `/Sep0 cs 0.5 scn`: half strength of the ink called Sep0.
//!
//! The alternate is what a proofing device shows. The name is what the press
//! reads, and it is the whole point: `PANTONE 185 C` on a plate of its own.

use tessera_color::Color;

/// One spot ink used in a document.
#[derive(Debug, Clone, PartialEq)]
pub struct Separation {
    /// The ink's name, as the press knows it.
    pub name: String,
    /// The colour it stands in as, at **full strength**, in whatever space the
    /// export writes.
    ///
    /// Full strength, not the tint it happened to be used at: the tint
    /// transform interpolates from nothing to this, and every use of the ink
    /// picks its own point along that line. Baking one use's tint in here would
    /// make every *other* use of the same ink wrong.
    pub alternate: Alternate,
}

/// The space a separation approximates itself in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Alternate {
    Rgb([f32; 3]),
    Cmyk([f32; 4]),
}

impl Alternate {
    /// The colour of no ink at all.
    ///
    /// White in RGB and nothing in CMYK — the paper, either way. This is the
    /// `C0` end of the tint transform, and getting it wrong makes a zero-tint
    /// spot print as solid ink.
    pub fn none(self) -> Vec<f32> {
        match self {
            Alternate::Rgb(_) => vec![1.0, 1.0, 1.0],
            Alternate::Cmyk(_) => vec![0.0, 0.0, 0.0, 0.0],
        }
    }

    /// The colour at full strength: the `C1` end.
    pub fn full(self) -> Vec<f32> {
        match self {
            Alternate::Rgb(v) => v.to_vec(),
            Alternate::Cmyk(v) => v.to_vec(),
        }
    }
}

/// Every spot ink a colour names, at full strength.
///
/// A spot inside a gradient stop or a stroke counts the same as one in a fill:
/// the plate exists either way.
pub fn spots_in(colour: &Color, ink: &crate::Ink) -> Option<Separation> {
    let Color::Spot { name, fallback, .. } = colour else {
        return None;
    };
    // **At full strength.** The tint on this particular use is applied where
    // the colour is set, not baked into the plate.
    let full = Color::Spot {
        name: name.clone(),
        tint: 1.0,
        fallback: fallback.clone(),
    };
    let alternate = match ink.components(&full) {
        crate::ink::Components::Rgb(v) => Alternate::Rgb(v),
        crate::ink::Components::Cmyk(v) => Alternate::Cmyk(v),
    };
    Some(Separation {
        name: name.clone(),
        alternate,
    })
}

/// How much ink a colour asks for, if it is a spot.
pub fn tint_of(colour: &Color) -> Option<f32> {
    match colour {
        Color::Spot { tint, .. } => Some(tint.clamp(0.0, 1.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ink;

    fn a_spot(tint: f32) -> Color {
        Color::Spot {
            name: "PANTONE 185 C".to_string(),
            tint,
            fallback: Box::new(Color::Cmyk {
                c: 0.0,
                m: 0.91,
                y: 0.76,
                k: 0.0,
                a: 1.0,
            }),
        }
    }

    #[test]
    fn a_spot_becomes_a_separation_named_for_its_ink() {
        // The name is what the press reads, and the whole point of the plate.
        let found = spots_in(&a_spot(1.0), &Ink::Rgb).expect("a separation");
        assert_eq!(found.name, "PANTONE 185 C");
    }

    #[test]
    fn the_plate_is_described_at_full_strength_whatever_tint_it_was_used_at() {
        // **The trap.** The tint transform runs from nothing to the ink; every
        // use picks its own point along it. Baking one use's tint into the
        // plate would make every other use of the same ink wrong — and the
        // wrongness would scale with how faint the first one happened to be.
        let full = spots_in(&a_spot(1.0), &Ink::Rgb).expect("a separation");
        let faint = spots_in(&a_spot(0.1), &Ink::Rgb).expect("a separation");
        assert_eq!(full.alternate, faint.alternate);
    }

    #[test]
    fn the_tint_is_read_from_the_colour_that_used_it() {
        assert_eq!(tint_of(&a_spot(0.4)), Some(0.4));
        assert_eq!(tint_of(&Color::BLACK), None);
    }

    #[test]
    fn a_tint_outside_its_range_is_clamped() {
        // A negative tint or one above full is ink a press cannot lay down, and
        // `scn` outside the domain is an operand a RIP may refuse outright.
        let mut wild = a_spot(4.0);
        assert_eq!(tint_of(&wild), Some(1.0));
        wild = a_spot(-2.0);
        assert_eq!(tint_of(&wild), Some(0.0));
    }

    #[test]
    fn no_ink_is_the_paper_in_either_space() {
        // The `C0` end of the transform. Getting it wrong makes a zero-tint
        // spot print as solid ink, which is the loudest possible failure.
        assert_eq!(Alternate::Rgb([0.5; 3]).none(), vec![1.0, 1.0, 1.0]);
        assert_eq!(Alternate::Cmyk([0.5; 4]).none(), vec![0.0; 4]);
    }

    #[test]
    fn a_process_colour_is_not_a_separation() {
        assert!(spots_in(&Color::BLACK, &Ink::Rgb).is_none());
    }
}
