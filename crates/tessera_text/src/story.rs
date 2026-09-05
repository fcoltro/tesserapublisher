//! The story model: text plus the formatting applied to it.

use std::ops::Range;

use serde::{Deserialize, Serialize};
use tessera_color::Color;

slotmap::new_key_type! {
    /// A named character style, held on the document.
    ///
    /// The id lives here because it is part of a story's vocabulary — a run
    /// refers to one — even though the arena that holds the styles belongs to
    /// the document. An id rather than a name, so renaming a style does not
    /// orphan every run that uses it.
    pub struct CharacterStyleId;
    /// A named paragraph style, held on the document.
    pub struct ParagraphStyleId;
}

/// How a run's letters are cased when drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Case {
    Normal,
    Upper,
    Lower,
    SmallCaps,
}

/// How a paragraph's lines sit within their measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alignment {
    Left,
    Centre,
    Right,
    Justify,
}

/// Character formatting, every field optional.
///
/// `None` means **inherit**. That is the whole mechanism behind a style that
/// cascades: a run holds a reference to a style and a set of overrides, never
/// a resolved copy, so there is nothing to go stale when the style changes.
///
/// The cascade is document default → paragraph style → character style →
/// local override, and [`CharacterFormat::over`] is one step of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterFormat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// CSS-style numeric weight: 400 regular, 700 bold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// Letter spacing, in thousandths of an em — the unit a typographer uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case: Option<Case>,
    /// Points above the baseline; negative sinks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_shift: Option<f32>,
    /// Multiple of the font size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Color>,
}

impl CharacterFormat {
    /// This format applied over `base`: what `self` states wins, what it
    /// leaves `None` is inherited.
    ///
    /// Associative, which is what lets a four-level cascade be folded in any
    /// order without changing the answer.
    pub fn over(&self, base: &CharacterFormat) -> CharacterFormat {
        CharacterFormat {
            family: self.family.clone().or_else(|| base.family.clone()),
            size: self.size.or(base.size),
            weight: self.weight.or(base.weight),
            italic: self.italic.or(base.italic),
            tracking: self.tracking.or(base.tracking),
            case: self.case.or(base.case),
            baseline_shift: self.baseline_shift.or(base.baseline_shift),
            line_height: self.line_height.or(base.line_height),
            colour: self.colour.clone().or_else(|| base.colour.clone()),
        }
    }

    /// Whether this format says nothing at all.
    pub fn is_empty(&self) -> bool {
        *self == CharacterFormat::default()
    }
}

/// Paragraph formatting, every field optional, plus the character formatting
/// a paragraph imposes before any run of its own speaks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParagraphFormat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<Alignment>,
    /// Points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_left: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_right: Option<f32>,
    /// Extra indent on the paragraph's first line; negative hangs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_first: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_before: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_after: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hyphenate: Option<bool>,
    /// What every run in the paragraph inherits before its own style speaks.
    #[serde(default)]
    pub character: CharacterFormat,
}

impl ParagraphFormat {
    pub fn over(&self, base: &ParagraphFormat) -> ParagraphFormat {
        ParagraphFormat {
            alignment: self.alignment.or(base.alignment),
            indent_left: self.indent_left.or(base.indent_left),
            indent_right: self.indent_right.or(base.indent_right),
            indent_first: self.indent_first.or(base.indent_first),
            space_before: self.space_before.or(base.space_before),
            space_after: self.space_after.or(base.space_after),
            hyphenate: self.hyphenate.or(base.hyphenate),
            character: self.character.over(&base.character),
        }
    }
}

/// One span of character formatting over a story's text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub range: Range<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<CharacterStyleId>,
    #[serde(default)]
    pub local: CharacterFormat,
}

impl Run {
    /// A run over `range` that states nothing of its own.
    pub fn plain(range: Range<usize>) -> Self {
        Self {
            range,
            style: None,
            local: CharacterFormat::default(),
        }
    }

    /// Whether two runs would draw identically, and so could be one run.
    pub fn same_formatting(&self, other: &Run) -> bool {
        self.style == other.style && self.local == other.local
    }
}

/// One span of paragraph formatting over a story's text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParagraphRun {
    pub range: Range<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<ParagraphStyleId>,
    #[serde(default)]
    pub local: ParagraphFormat,
}

/// Character formatting.
///
/// Milestone 0 applies one style to a whole story. Milestone 2 splits this
/// into runs, which is an additive change: a story gains a `runs` vector and
/// this becomes the default for spans that do not override it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub family: String,
    /// Points.
    pub size: f32,
    /// Multiple of the font size.
    pub line_height: f32,
    pub color: Color,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            // A generic family rather than a named one, so a document opens
            // the same way on a machine without Arial. Milestone 2 adds real
            // family resolution with a visible warning when one is missing.
            family: "sans-serif".to_string(),
            size: 12.0,
            line_height: 1.2,
            color: Color::BLACK,
        }
    }
}

/// A story exists once and is addressed by `StoryId`, independent of the
/// frames that display it. Milestone 4 threads one story through several.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Story {
    pub text: String,
    /// The whole story's formatting, as milestone 0 had it.
    ///
    /// Still what the shaper reads. Phase 2 of milestone 2 turns it into the
    /// first run and retires it; until then it is the one source of truth and
    /// `runs` is built but not yet consulted, so the two cannot disagree
    /// about anything on screen.
    pub style: TextStyle,

    /// Character formatting, span by span.
    ///
    /// Sorted, non-overlapping, and covering exactly `[0, text.len())`. See
    /// [`Story::runs_are_sound`], which is the invariant every operation here
    /// preserves — a run list that drifts out of step with the text is
    /// corruption rather than a glitch, and its symptom appears far from its
    /// cause.
    #[serde(default)]
    pub runs: Vec<Run>,

    /// Paragraph formatting, under the same invariant.
    #[serde(default)]
    pub paragraphs: Vec<ParagraphRun>,
}

impl Story {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let runs = if text.is_empty() {
            Vec::new()
        } else {
            vec![Run::plain(0..text.len())]
        };
        let paragraphs = if text.is_empty() {
            Vec::new()
        } else {
            vec![ParagraphRun {
                range: 0..text.len(),
                style: None,
                local: ParagraphFormat::default(),
            }]
        };
        Self {
            text,
            style: TextStyle::default(),
            runs,
            paragraphs,
        }
    }

    /// Whether the run lists still describe this text.
    ///
    /// Sorted, non-overlapping, gap-free, covering exactly the text — and
    /// empty only when the text is. Checked by a property test over generated
    /// edit sequences, which is the only way to be sure the arithmetic below
    /// holds under every ordering.
    pub fn runs_are_sound(&self) -> bool {
        fn sound(ranges: &[Range<usize>], len: usize) -> bool {
            if ranges.is_empty() {
                return len == 0;
            }
            if ranges[0].start != 0 {
                return false;
            }
            for pair in ranges.windows(2) {
                if pair[0].end != pair[1].start || pair[0].start >= pair[0].end {
                    return false;
                }
            }
            let last = &ranges[ranges.len() - 1];
            last.start < last.end && last.end == len
        }

        let n = self.text.len();
        let runs: Vec<_> = self.runs.iter().map(|r| r.range.clone()).collect();
        let paras: Vec<_> = self.paragraphs.iter().map(|p| p.range.clone()).collect();
        sound(&runs, n) && sound(&paras, n)
    }

    /// The run covering `offset`, if any.
    pub fn run_at(&self, offset: usize) -> Option<&Run> {
        self.runs.iter().find(|r| r.range.contains(&offset))
    }

    /// Insert text, carrying the runs with it.
    ///
    /// Inserting inside a run extends it. Inserting at a boundary joins the
    /// run to the **left** — what every editor does, and what makes typing
    /// after a bold word continue bold.
    pub fn insert_text(&mut self, at: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let at = at.min(self.text.len());
        let n = text.len();
        self.text.insert_str(at, text);

        if self.runs.is_empty() {
            self.runs = vec![Run::plain(0..self.text.len())];
            self.paragraphs = vec![ParagraphRun {
                range: 0..self.text.len(),
                style: None,
                local: ParagraphFormat::default(),
            }];
            return;
        }

        grow(self.runs.iter_mut().map(|r| &mut r.range).collect(), at, n);
        grow(
            self.paragraphs.iter_mut().map(|p| &mut p.range).collect(),
            at,
            n,
        );
    }

    /// Delete a range, carrying the runs with it.
    ///
    /// Runs the range wholly covers are dropped; the ones it straddles are
    /// clipped. Neighbours that would draw identically are then merged, or
    /// the list grows without bound over a long editing session.
    pub fn delete_range(&mut self, range: Range<usize>) {
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len());
        if start >= end {
            return;
        }
        self.text.replace_range(start..end, "");

        self.runs = shrink(
            std::mem::take(&mut self.runs),
            start,
            end,
            |r| &r.range,
            |mut r, g| {
                r.range = g;
                r
            },
        );
        self.paragraphs = shrink(
            std::mem::take(&mut self.paragraphs),
            start,
            end,
            |p| &p.range,
            |mut p, g| {
                p.range = g;
                p
            },
        );

        self.merge_equal_neighbours();
    }

    /// Fold together adjacent runs that would draw the same.
    pub fn merge_equal_neighbours(&mut self) {
        let mut merged: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in std::mem::take(&mut self.runs) {
            match merged.last_mut() {
                Some(previous) if previous.same_formatting(&run) => {
                    previous.range.end = run.range.end;
                }
                _ => merged.push(run),
            }
        }
        self.runs = merged;

        let mut folded: Vec<ParagraphRun> = Vec::with_capacity(self.paragraphs.len());
        for para in std::mem::take(&mut self.paragraphs) {
            match folded.last_mut() {
                Some(previous) if previous.style == para.style && previous.local == para.local => {
                    previous.range.end = para.range.end;
                }
                _ => folded.push(para),
            }
        }
        self.paragraphs = folded;
    }
}

/// Extend whichever span holds `at` by `n`, and shift the rest along.
///
/// The span *ending* at `at` wins over the one starting there, which is the
/// join-left rule.
fn grow(all: Vec<&mut Range<usize>>, at: usize, n: usize) {
    let target = all
        .iter()
        .position(|r| r.start < at && at <= r.end)
        .unwrap_or(0);

    for (i, range) in all.into_iter().enumerate() {
        if i < target {
            continue;
        }
        if i == target {
            range.end += n;
        } else {
            range.start += n;
            range.end += n;
        }
    }
}

/// Clip a list of spans against a deleted range.
fn shrink<T>(
    items: Vec<T>,
    start: usize,
    end: usize,
    get: impl Fn(&T) -> &Range<usize>,
    set: impl Fn(T, Range<usize>) -> T,
) -> Vec<T> {
    let n = end - start;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let r = get(&item).clone();
        let clip = |v: usize| {
            if v <= start {
                v
            } else if v >= end {
                v - n
            } else {
                start
            }
        };
        let (a, b) = (clip(r.start), clip(r.end));
        if a < b {
            out.push(set(item, a..b));
        }
    }
    out
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use proptest::prelude::*;

    fn bold() -> CharacterFormat {
        CharacterFormat {
            weight: Some(700),
            ..CharacterFormat::default()
        }
    }

    // --- the cascade ---------------------------------------------------

    #[test]
    fn an_empty_format_inherits_everything() {
        let base = bold();
        assert_eq!(CharacterFormat::default().over(&base), base);
    }

    #[test]
    fn a_stated_field_wins_over_an_inherited_one() {
        let base = bold();
        let light = CharacterFormat {
            weight: Some(300),
            ..CharacterFormat::default()
        };
        assert_eq!(light.over(&base).weight, Some(300));
    }

    #[test]
    fn the_cascade_folds_the_same_in_either_order() {
        // Four levels deep, so it has to be associative or the answer would
        // depend on how the fold happened to be written.
        let a = CharacterFormat {
            size: Some(11.0),
            ..CharacterFormat::default()
        };
        let b = CharacterFormat {
            size: Some(12.0),
            italic: Some(true),
            ..CharacterFormat::default()
        };
        let c = CharacterFormat {
            size: Some(13.0),
            italic: Some(false),
            weight: Some(700),
            ..CharacterFormat::default()
        };
        assert_eq!(a.over(&b).over(&c), a.over(&b.over(&c)));
    }

    // --- the invariant -------------------------------------------------

    #[test]
    fn a_new_story_is_covered_by_one_run() {
        let story = Story::new("hello");
        assert!(story.runs_are_sound());
        assert_eq!(story.runs.len(), 1);
        assert_eq!(story.runs[0].range, 0..5);
    }

    #[test]
    fn an_empty_story_has_no_runs_rather_than_one_empty_run() {
        let story = Story::new("");
        assert!(story.runs_are_sound());
        assert!(story.runs.is_empty());
    }

    #[test]
    fn run_at_finds_the_run_holding_an_offset_and_nothing_past_the_end() {
        let story = Story::new("hello");
        assert!(story.run_at(0).is_some());
        assert!(story.run_at(4).is_some());
        assert!(story.run_at(5).is_none(), "the end is not inside anything");
    }

    #[test]
    fn the_invariant_rejects_a_gap() {
        let mut story = Story::new("hello");
        story.runs = vec![Run::plain(0..2), Run::plain(3..5)];
        assert!(!story.runs_are_sound());
    }

    #[test]
    fn the_invariant_rejects_an_overlap() {
        let mut story = Story::new("hello");
        story.runs = vec![Run::plain(0..3), Run::plain(2..5)];
        assert!(!story.runs_are_sound());
    }

    #[test]
    fn the_invariant_rejects_a_list_that_stops_short_of_the_text() {
        let mut story = Story::new("hello");
        story.runs = vec![Run::plain(0..3)];
        assert!(!story.runs_are_sound());
    }

    // --- edits carry the runs ------------------------------------------

    #[test]
    fn inserting_inside_a_run_extends_it() {
        let mut story = Story::new("hello");
        story.insert_text(2, "XX");
        assert_eq!(story.text, "heXXllo");
        assert!(story.runs_are_sound());
        assert_eq!(story.runs.len(), 1);
    }

    #[test]
    fn inserting_at_a_boundary_joins_the_run_to_the_left() {
        // What makes typing after a bold word continue bold.
        let mut story = Story::new("ab");
        story.runs = vec![
            Run {
                range: 0..1,
                style: None,
                local: bold(),
            },
            Run::plain(1..2),
        ];
        story.insert_text(1, "X");

        assert!(story.runs_are_sound());
        assert_eq!(story.runs[0].range, 0..2, "the bold run took the new text");
        assert_eq!(story.runs[0].local, bold());
    }

    #[test]
    fn inserting_into_an_empty_story_creates_the_covering_run() {
        let mut story = Story::new("");
        story.insert_text(0, "hello");
        assert!(story.runs_are_sound());
        assert_eq!(story.runs.len(), 1);
        assert_eq!(story.runs[0].range, 0..5);
    }

    #[test]
    fn deleting_drops_the_runs_it_covers_and_clips_the_ones_it_straddles() {
        let mut story = Story::new("abcdef");
        story.runs = vec![
            Run::plain(0..2),
            Run {
                range: 2..4,
                style: None,
                local: bold(),
            },
            Run::plain(4..6),
        ];
        // Takes all of the middle run and one byte either side.
        story.delete_range(1..5);

        assert_eq!(story.text, "af");
        assert!(story.runs_are_sound());
        assert!(
            story.runs.iter().all(|r| r.local != bold()),
            "the wholly covered run should be gone"
        );
    }

    #[test]
    fn deleting_everything_leaves_no_runs() {
        let mut story = Story::new("hello");
        story.delete_range(0..5);
        assert_eq!(story.text, "");
        assert!(story.runs.is_empty());
        assert!(story.runs_are_sound());
    }

    #[test]
    fn neighbours_that_would_draw_the_same_are_merged() {
        // Without this the list grows without bound over a long session.
        let mut story = Story::new("abcd");
        story.runs = vec![Run::plain(0..2), Run::plain(2..4)];
        story.delete_range(0..1);

        assert!(story.runs_are_sound());
        assert_eq!(story.runs.len(), 1, "two identical runs should be one");
    }

    #[test]
    fn neighbours_that_differ_are_not_merged() {
        let mut story = Story::new("abcd");
        story.runs = vec![
            Run::plain(0..2),
            Run {
                range: 2..4,
                style: None,
                local: bold(),
            },
        ];
        story.delete_range(0..1);

        assert!(story.runs_are_sound());
        assert_eq!(story.runs.len(), 2);
    }

    // --- the property that matters -------------------------------------

    /// One edit, as the generator produces them.
    #[derive(Debug, Clone)]
    enum Edit {
        Insert(usize, String),
        Delete(usize, usize),
    }

    fn an_edit() -> impl Strategy<Value = Edit> {
        prop_oneof![
            (0usize..40, "[a-z]{0,4}").prop_map(|(at, t)| Edit::Insert(at, t)),
            (0usize..40, 0usize..40).prop_map(|(a, b)| Edit::Delete(a.min(b), a.max(b))),
        ]
    }

    proptest! {
        /// The invariant holds through any sequence of edits.
        ///
        /// This is the whole reason phase 1 exists before any interface: runs
        /// that drift out of step with the text are corruption, and the
        /// symptom shows up far from the cause.
        #[test]
        fn runs_stay_sound_through_any_sequence_of_edits(
            edits in proptest::collection::vec(an_edit(), 0..30)
        ) {
            let mut story = Story::new("the quick brown fox");
            prop_assert!(story.runs_are_sound());

            for edit in edits {
                match edit {
                    Edit::Insert(at, text) => {
                        // Only insert on a character boundary; the caller is
                        // the editing buffer, which never does otherwise.
                        let at = at.min(story.text.len());
                        if story.text.is_char_boundary(at) {
                            story.insert_text(at, &text);
                        }
                    }
                    Edit::Delete(a, b) => {
                        let a = a.min(story.text.len());
                        let b = b.min(story.text.len());
                        if story.text.is_char_boundary(a) && story.text.is_char_boundary(b) {
                            story.delete_range(a..b);
                        }
                    }
                }
                prop_assert!(
                    story.runs_are_sound(),
                    "runs {:?} no longer describe {:?}",
                    story.runs,
                    story.text
                );
            }
        }
    }
}
