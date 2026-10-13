//! Optical margin alignment: punctuation and the lighter capitals set a
//! little past the edge of the measure, so the column's edge looks straight.
//!
//! A column set flush is flush by the metrics, not to the eye. A full stop or
//! a hyphen at the end of a line is mostly white, and a quotation mark at the
//! start of one is a speck high above the baseline; the edge reads as dented
//! wherever they fall. InDesign's Optical Margin Alignment moves them out
//! into the margin by a share of their own width, and so does this.
//!
//! The table is each character's share of its advance that may stand outside
//! the left edge and outside the right. It is DesignCraft's
//! (`crates/compose/src/breaker.rs`, `hang`), used under its MIT licence; see
//! `ATTRIBUTION.md`.

/// How much of `c`'s advance may stand outside the measure: (left edge,
/// right edge), each a fraction of the advance. `(0.0, 0.0)` for everything
/// that hangs not at all, which is most of the alphabet.
pub fn hang(c: char) -> (f64, f64) {
    match c {
        '.' | ',' => (0.0, 1.0),
        '-' | '\u{2010}' | '\u{2011}' | '\u{AD}' => (0.75, 0.75),
        '\'' | '"' | '‘' | '’' | '“' | '”' | '‚' | '„' | '‹' | '›' | '«' | '»' => {
            (1.0, 1.0)
        }
        '\u{2013}' | '\u{2014}' => (0.5, 0.5),
        ':' | ';' => (0.0, 0.5),
        '!' | '?' => (0.0, 0.2),
        '*' => (0.3, 0.3),
        '(' | '[' | '{' => (0.1, 0.0),
        ')' | ']' | '}' => (0.0, 0.1),
        'A' | 'V' | 'W' | 'Y' | 'T' | 'v' | 'w' | 'y' => (0.05, 0.05),
        'O' | 'C' | 'G' | 'Q' | 'o' | 'c' => (0.03, 0.03),
        _ => (0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_set_flush_and_punctuation_hangs() {
        assert_eq!(hang('H'), (0.0, 0.0));
        assert_eq!(
            hang('.').1,
            1.0,
            "a full stop hangs all the way out on the right"
        );
        assert_eq!(hang('.').0, 0.0, "and not at all on the left");
        assert_eq!(
            hang('“').0,
            1.0,
            "an opening quote hangs all the way out on the left"
        );
        assert!(hang('-').1 > 0.5);
    }
}
