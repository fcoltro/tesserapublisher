//! Footnote options: how the notes are numbered and set, document-wide.
//!
//! InDesign's Document Footnote Options, the parts a person reaches for:
//! what the numbers count in, where they restart, and how the notes sit at
//! the foot of the column — the space above them, the space between them,
//! and the rule. One struct on the document, replaced whole by the dialog,
//! so a change is one undo entry.

use serde::{Deserialize, Serialize};

/// What a footnote's number counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FootnoteNumbering {
    #[default]
    Arabic,
    LowerRoman,
    UpperRoman,
    LowerAlpha,
    UpperAlpha,
    /// `* † ‡ §`, then doubled, then tripled — the old way, for a page with
    /// a note or two.
    Symbols,
}

impl FootnoteNumbering {
    /// The `n`th note's label, counting from one.
    pub fn label(self, n: u32) -> String {
        use tessera_text::story::Numbering;
        let n = n.max(1) as usize;
        match self {
            FootnoteNumbering::Arabic => Numbering::Arabic.label(n),
            FootnoteNumbering::LowerRoman => Numbering::LowerRoman.label(n),
            FootnoteNumbering::UpperRoman => Numbering::UpperRoman.label(n),
            FootnoteNumbering::LowerAlpha => Numbering::LowerAlpha.label(n),
            FootnoteNumbering::UpperAlpha => Numbering::UpperAlpha.label(n),
            FootnoteNumbering::Symbols => {
                const MARKS: [char; 4] = ['*', '\u{2020}', '\u{2021}', '\u{00A7}'];
                let mark = MARKS[(n - 1) % MARKS.len()];
                let times = (n - 1) / MARKS.len() + 1;
                std::iter::repeat_n(mark, times).collect()
            }
        }
    }
}

/// Where the count starts again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Restart {
    /// Once through the story: 1, 2, 3 across every page it runs over.
    #[default]
    Never,
    /// From one again on each page the story reaches. Measured by the
    /// frames: the count restarts in the first frame of the thread that
    /// stands on a page.
    Page,
}

/// Where the notes are set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NotePlacement {
    /// At the foot of the column that cites them: footnotes.
    #[default]
    Foot,
    /// Gathered into one story — Layout ▸ Endnotes… places and updates it —
    /// with nothing at the foot: endnotes. The references in the text read
    /// the same; the count runs through the story, whatever [`Restart`]
    /// says, because a list at the end has no pages to restart on.
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FootnoteOptions {
    #[serde(default)]
    pub numbering: FootnoteNumbering,
    #[serde(default)]
    pub restart: Restart,
    #[serde(default)]
    pub placement: NotePlacement,
    /// What the first note is numbered.
    #[serde(default = "one")]
    pub start_at: u32,
    /// Air between the last line of copy and the rule above the notes.
    #[serde(default = "default_space_before")]
    pub space_before: f64,
    /// Air between one note and the next.
    #[serde(default)]
    pub space_between: f64,
    /// Whether a rule is drawn above the first note.
    #[serde(default = "yes")]
    pub rule: bool,
    #[serde(default = "default_rule_weight")]
    pub rule_weight: f64,
    /// How much of the column's width the rule runs across, 0 to 1.
    #[serde(default = "default_rule_fraction")]
    pub rule_fraction: f64,
}

fn one() -> u32 {
    1
}
fn yes() -> bool {
    true
}
fn default_space_before() -> f64 {
    6.0
}
fn default_rule_weight() -> f64 {
    0.5
}
fn default_rule_fraction() -> f64 {
    0.33
}

impl Default for FootnoteOptions {
    fn default() -> Self {
        Self {
            numbering: FootnoteNumbering::Arabic,
            restart: Restart::Never,
            placement: NotePlacement::Foot,
            start_at: one(),
            space_before: default_space_before(),
            space_between: 0.0,
            rule: yes(),
            rule_weight: default_rule_weight(),
            rule_fraction: default_rule_fraction(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_numbering_writes_its_first_notes() {
        assert_eq!(FootnoteNumbering::Arabic.label(3), "3");
        assert_eq!(FootnoteNumbering::LowerRoman.label(4), "iv");
        assert_eq!(FootnoteNumbering::UpperAlpha.label(2), "B");
        assert_eq!(FootnoteNumbering::Symbols.label(1), "*");
        assert_eq!(FootnoteNumbering::Symbols.label(4), "\u{00A7}");
        assert_eq!(FootnoteNumbering::Symbols.label(5), "**");
        assert_eq!(
            FootnoteNumbering::Symbols.label(10),
            "\u{2020}\u{2020}\u{2020}"
        );
    }

    #[test]
    fn the_defaults_are_what_the_notes_were_set_with_before_there_were_options() {
        let o = FootnoteOptions::default();
        assert_eq!(o.space_before, 6.0);
        assert!(o.rule);
        assert_eq!(o.rule_weight, 0.5);
        assert_eq!(o.rule_fraction, 0.33);
        assert_eq!(o.start_at, 1);
    }
}
