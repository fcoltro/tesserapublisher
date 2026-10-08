//! Units of measure.
//!
//! The document stores points and nothing else. This type converts at the
//! edges — parsing what a user types and formatting what a field shows — so
//! that no conversion is scattered through the interface.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Unit {
    Millimetres,
    Points,
    Pixels,
    Inches,
    Picas,
    Centimetres,
    /// Twelve Didot points: the European pica.
    Ciceros,
    /// A newspaper's column-inch fourteenth.
    Agates,
}

/// A Didot point, in PostScript points: 0.376 mm, as InDesign has it.
const DIDOT: f64 = 1.065_74;

impl Unit {
    /// Every unit, for iteration in tests and in a unit picker.
    /// In InDesign's order, which is the one its ruler menu lists them in.
    pub const ALL: [Unit; 8] = [
        Unit::Points,
        Unit::Picas,
        Unit::Inches,
        Unit::Millimetres,
        Unit::Centimetres,
        Unit::Ciceros,
        Unit::Agates,
        Unit::Pixels,
    ];

    /// How many points one of this unit is worth.
    ///
    /// A pixel is 1/72 inch, so it equals a point. That is PDF's user space
    /// and the honest choice for a tool whose output is print; the 96-per-inch
    /// pixel the web uses would make on-screen numbers disagree with exported
    /// ones.
    pub fn points_per(self) -> f64 {
        match self {
            Unit::Points | Unit::Pixels => 1.0,
            Unit::Inches => 72.0,
            Unit::Picas => 12.0,
            Unit::Millimetres => 72.0 / 25.4,
            Unit::Centimetres => 72.0 / 2.54,
            Unit::Ciceros => 12.0 * DIDOT,
            Unit::Agates => 5.14,
        }
    }

    pub fn to_points(self, value: f64) -> f64 {
        value * self.points_per()
    }

    pub fn from_points(self, points: f64) -> f64 {
        points / self.points_per()
    }

    pub fn suffix(self) -> &'static str {
        match self {
            Unit::Millimetres => "mm",
            Unit::Points => "pt",
            Unit::Pixels => "px",
            Unit::Inches => "in",
            Unit::Picas => "p",
            Unit::Centimetres => "cm",
            Unit::Ciceros => "c",
            Unit::Agates => "ag",
        }
    }

    /// The unit's name in full, as a menu lists it.
    pub fn name(self) -> &'static str {
        match self {
            Unit::Millimetres => "Millimetres",
            Unit::Points => "Points",
            Unit::Pixels => "Pixels",
            Unit::Inches => "Inches",
            Unit::Picas => "Picas",
            Unit::Centimetres => "Centimetres",
            Unit::Ciceros => "Ciceros",
            Unit::Agates => "Agates",
        }
    }

    /// How far one press of a field's stepper moves it, in this unit: one of
    /// it, but a sixteenth of an inch, a tenth of a centimetre.
    pub fn step(self) -> f64 {
        match self {
            Unit::Inches => 0.0625,
            Unit::Centimetres => 0.1,
            _ => 1.0,
        }
    }

    /// Read a field's text as a measurement in points.
    ///
    /// A bare number is in `current`. A suffix overrides it. `1p6` is the
    /// compositor's picas-and-points notation. Anything else is rejected —
    /// guessing at input the user did not mean is how a layout silently moves.
    pub fn parse_to_points(text: &str, current: Unit) -> Option<f64> {
        let text = text.trim().to_ascii_lowercase();
        if text.is_empty() {
            return None;
        }

        // Picas-and-points, and ciceros-and-Didot-points the same way (1c6),
        // before the suffix table, because `p` and `c` are infixes here
        // rather than suffixes. Both halves must be plain digits, which is
        // what keeps `px`, `pt` and `cm` out of this branch.
        for (mark, whole, part) in [('p', 12.0, 1.0), ('c', 12.0 * DIDOT, DIDOT)] {
            if let Some((big, small)) = text.split_once(mark) {
                // "3 p" is how a field writes three picas.
                let (big, small) = (big.trim(), small.trim());
                let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit() || c == '.');
                if digits(big) && small.chars().all(|c| c.is_ascii_digit()) {
                    let big: f64 = if big.is_empty() {
                        0.0
                    } else {
                        big.parse().ok()?
                    };
                    let small: f64 = if small.is_empty() {
                        0.0
                    } else {
                        small.parse().ok()?
                    };
                    return Some(big * whole + small * part);
                }
            }
        }

        for unit in [
            Unit::Millimetres,
            Unit::Centimetres,
            Unit::Points,
            Unit::Pixels,
            Unit::Inches,
            Unit::Agates,
        ] {
            if let Some(number) = text.strip_suffix(unit.suffix()) {
                return Some(unit.to_points(number.trim().parse().ok()?));
            }
        }

        Some(current.to_points(text.parse().ok()?))
    }

    /// Render a measurement for a field, without trailing zeroes.
    pub fn format(self, points: f64) -> String {
        let value = self.from_points(points);
        let mut text = format!("{value:.3}");
        if text.contains('.') {
            text = text.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        format!("{text} {}", self.suffix())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inch_is_seventy_two_points() {
        assert_eq!(Unit::Inches.to_points(1.0), 72.0);
    }

    #[test]
    fn a_millimetre_is_the_metric_share_of_an_inch() {
        let expected = 72.0 / 25.4;
        assert!((Unit::Millimetres.to_points(1.0) - expected).abs() < 1e-12);
    }

    #[test]
    fn a_cicero_is_twelve_didot_points_and_an_agate_five_and_a_bit() {
        assert!((Unit::Ciceros.to_points(1.0) - 12.788_88).abs() < 1e-6);
        assert_eq!(Unit::Agates.to_points(1.0), 5.14);
        assert!((Unit::Centimetres.to_points(1.0) - 72.0 / 2.54).abs() < 1e-12);
        assert_eq!(
            Unit::parse_to_points("2cm", Unit::Points),
            Some(Unit::Centimetres.to_points(2.0))
        );
        assert_eq!(
            Unit::parse_to_points("1c6", Unit::Points),
            Some(18.0 * 1.065_74)
        );
        assert_eq!(
            Unit::parse_to_points("14ag", Unit::Points),
            Some(14.0 * 5.14)
        );
        // Every suffix reads back as its own unit.
        for unit in Unit::ALL {
            let written = unit.format(unit.to_points(3.0));
            let read = Unit::parse_to_points(&written, Unit::Points).expect("reads");
            assert!(
                (read - unit.to_points(3.0)).abs() < 1e-6,
                "{unit:?}: {written}"
            );
        }
    }

    #[test]
    fn a_pica_is_twelve_points() {
        assert_eq!(Unit::Picas.to_points(1.0), 12.0);
    }

    #[test]
    fn every_unit_round_trips_through_points() {
        for unit in Unit::ALL {
            for value in [0.0, 1.0, 12.5, -3.25, 1234.5678] {
                let there_and_back = unit.from_points(unit.to_points(value));
                assert!(
                    (there_and_back - value).abs() < 1e-9,
                    "{unit:?} lost {value} (got {there_and_back})"
                );
            }
        }
    }

    #[test]
    fn a_bare_number_is_read_in_the_current_unit() {
        assert_eq!(Unit::parse_to_points("10", Unit::Picas), Some(120.0));
    }

    #[test]
    fn a_suffix_overrides_the_current_unit() {
        assert_eq!(Unit::parse_to_points("1in", Unit::Millimetres), Some(72.0));
        assert_eq!(Unit::parse_to_points("12 pt", Unit::Inches), Some(12.0));
        assert_eq!(Unit::parse_to_points("72px", Unit::Millimetres), Some(72.0));
    }

    #[test]
    fn picas_and_points_are_written_together() {
        // 1p6 is one pica and six points, which is the notation a compositor
        // uses and the one InDesign accepts.
        assert_eq!(Unit::parse_to_points("1p6", Unit::Points), Some(18.0));
        assert_eq!(Unit::parse_to_points("3p", Unit::Points), Some(36.0));
        assert_eq!(Unit::parse_to_points("p6", Unit::Points), Some(6.0));
    }

    #[test]
    fn leading_dots_and_whitespace_are_accepted() {
        assert_eq!(Unit::parse_to_points("  .5in ", Unit::Points), Some(36.0));
    }

    #[test]
    fn nonsense_is_rejected_rather_than_guessed() {
        assert_eq!(Unit::parse_to_points("", Unit::Points), None);
        assert_eq!(Unit::parse_to_points("wide", Unit::Points), None);
        assert_eq!(Unit::parse_to_points("12qq", Unit::Points), None);
    }

    #[test]
    fn formatting_trims_the_noise_off_a_round_number() {
        assert_eq!(
            Unit::Millimetres.format(Unit::Millimetres.to_points(210.0)),
            "210 mm"
        );
        assert_eq!(Unit::Points.format(12.5), "12.5 pt");
    }
}
