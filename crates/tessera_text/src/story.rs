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

/// A named character style.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CharacterStyle {
    pub name: String,
    pub format: CharacterFormat,
}

/// A named paragraph style.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParagraphStyle {
    pub name: String,
    /// Including the character formatting the paragraph imposes, which lives in
    /// [`ParagraphFormat::character`].
    ///
    /// There was briefly a second `character` field here as well. Nothing read
    /// it: [`Story::resolve_run`] folds in `format.character`, so a style whose
    /// size was set on the other field silently did nothing. One place or the
    /// cascade cannot say which wins.
    pub format: ParagraphFormat,
}

/// Where the named styles live.
///
/// A trait rather than a type, because the tables belong to the document and
/// this crate has no dependency on the document tree — that isolation is what
/// lets cursor movement, selection and shaping be tested headless. The
/// document implements this; [`NoStyles`] serves the tests and the caret,
/// which have a story and no document.
pub trait Styles {
    fn character(&self, id: CharacterStyleId) -> Option<&CharacterFormat>;
    fn paragraph(&self, id: ParagraphStyleId) -> Option<&ParagraphFormat>;
    /// The floor of the cascade.
    fn document_default(&self) -> CharacterFormat;
}

/// No named styles at all, and a plain default.
///
/// What the caret and most tests want: a story resolves to its own local
/// formatting over a sensible floor.
pub struct NoStyles {
    pub default: CharacterFormat,
}

impl Default for NoStyles {
    fn default() -> Self {
        Self {
            default: CharacterFormat {
                family: Some("sans-serif".to_string()),
                size: Some(12.0),
                line_height: Some(1.2),
                colour: Some(Color::BLACK),
                ..CharacterFormat::default()
            },
        }
    }
}

impl Styles for NoStyles {
    fn character(&self, _: CharacterStyleId) -> Option<&CharacterFormat> {
        None
    }
    fn paragraph(&self, _: ParagraphStyleId) -> Option<&ParagraphFormat> {
        None
    }
    fn document_default(&self) -> CharacterFormat {
        self.default.clone()
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

/// The formatting a document falls back to.
///
/// Milestone 0 kept one of these per story. Milestone 2 moved formatting into
/// runs and this became the document's own floor — every field stated, so
/// anything resolved over it is complete.
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

    /// What a run actually draws as, through the whole cascade.
    ///
    /// The order is fixed here and nowhere else:
    ///
    /// ```text
    /// document default -> paragraph style -> paragraph local
    ///                  -> character style -> run local
    /// ```
    ///
    /// A run or paragraph naming a style that no longer exists falls through
    /// to the next level rather than failing. A deleted style should leave
    /// text looking plainer, never leave it unopenable.
    pub fn resolve_run(&self, run: &Run, styles: &dyn Styles) -> CharacterFormat {
        let mut format = styles.document_default();

        // The paragraph this run begins in, if any.
        if let Some(para) = self
            .paragraphs
            .iter()
            .find(|p| p.range.contains(&run.range.start))
        {
            if let Some(id) = para.style
                && let Some(style) = styles.paragraph(id)
            {
                format = style.character.over(&format);
            }
            format = para.local.character.over(&format);
        }

        if let Some(id) = run.style
            && let Some(style) = styles.character(id)
        {
            format = style.over(&format);
        }

        run.local.over(&format)
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

    /// Replace the whole text, keeping the formatting the story opened with.
    ///
    /// Writing `text` directly leaves `runs` describing a length the text no
    /// longer has, which [`Story::runs_are_sound`] calls corruption rather
    /// than a glitch. The first run's and first paragraph's formatting is
    /// kept, because a caller replacing the content has said nothing about
    /// the formatting.
    pub fn set_text(&mut self, text: impl Into<String>) {
        let run = self.runs.first().cloned();
        let paragraph = self.paragraphs.first().cloned();
        *self = Story::new(text);
        if let (Some(keep), Some(first)) = (run, self.runs.first_mut()) {
            first.style = keep.style;
            first.local = keep.local;
        }
        if let (Some(keep), Some(first)) = (paragraph, self.paragraphs.first_mut()) {
            first.style = keep.style;
            first.local = keep.local;
        }
    }

    /// Merge `format` into every run the range covers.
    ///
    /// The new format **wins**: setting bold over text that is 9pt italic
    /// leaves it 9pt italic bold. A `None` field means "do not touch this
    /// property", which is what lets the inspector change one control without
    /// flattening the rest — and is why applying an empty format is a no-op
    /// rather than a reset.
    ///
    /// Runs are split at both edges of the range, so a format applied to the
    /// middle of one run turns it into three. Identical neighbours are folded
    /// afterwards, or the list grows without bound over a session of small
    /// edits.
    pub fn apply_character_format(&mut self, range: Range<usize>, format: &CharacterFormat) {
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len());
        if start >= end || format.is_empty() {
            return;
        }

        split_span_at(&mut self.runs, start, |r| &mut r.range);
        split_span_at(&mut self.runs, end, |r| &mut r.range);

        for run in &mut self.runs {
            if run.range.start >= start && run.range.end <= end {
                run.local = format.over(&run.local);
            }
        }

        self.merge_equal_neighbours();
    }

    /// Attach a named character style to every run the range covers.
    ///
    /// `None` detaches. The runs' own overrides are left alone: a style is the
    /// floor a run sits on, so applying one must not silently discard the size
    /// somebody set by hand — InDesign shows those as the style "plus"
    /// overrides, and clearing them is a separate act.
    pub fn set_character_style(
        &mut self,
        range: Range<usize>,
        style: Option<CharacterStyleId>,
    ) {
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len());
        if start >= end {
            return;
        }
        split_span_at(&mut self.runs, start, |r| &mut r.range);
        split_span_at(&mut self.runs, end, |r| &mut r.range);
        for run in &mut self.runs {
            if run.range.start >= start && run.range.end <= end {
                run.style = style;
            }
        }
        self.merge_equal_neighbours();
    }

    /// Attach a named paragraph style to every paragraph the range touches.
    ///
    /// Widened to whole paragraphs, the same way the formatting is.
    pub fn set_paragraph_style(
        &mut self,
        range: Range<usize>,
        style: Option<ParagraphStyleId>,
    ) {
        if self.text.is_empty() {
            return;
        }
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len()).max(start);
        let bounds = self.paragraph_bounds(start..end);

        split_span_at(&mut self.paragraphs, bounds.start, |p| &mut p.range);
        split_span_at(&mut self.paragraphs, bounds.end, |p| &mut p.range);
        // See `apply_paragraph_format`: a run must not straddle a paragraph
        // boundary, or its text on the far side takes the wrong style.
        split_span_at(&mut self.runs, bounds.start, |r| &mut r.range);
        split_span_at(&mut self.runs, bounds.end, |r| &mut r.range);
        for para in &mut self.paragraphs {
            if para.range.start >= bounds.start && para.range.end <= bounds.end {
                para.style = style;
            }
        }
        self.merge_equal_neighbours();
    }

    /// Remove every reference to a character style, keeping the appearance.
    ///
    /// What deleting a style does. The style's format is merged **under** each
    /// run's own overrides — the same precedence the version 6-to-7 migration
    /// used when it folded a story's single style into its runs — so a run that
    /// stated a size keeps it and one that said nothing inherits what the style
    /// said. Then the reference goes.
    ///
    /// The alternative is letting the text fall back to the document default,
    /// which throws away work to save a fold.
    pub fn flatten_character_style(&mut self, id: CharacterStyleId, format: &CharacterFormat) {
        for run in &mut self.runs {
            if run.style != Some(id) {
                continue;
            }
            run.local = run.local.over(format);
            run.style = None;
        }
        self.merge_equal_neighbours();
    }

    /// As above, for a paragraph style.
    ///
    /// A paragraph style carries character formatting too, in
    /// [`ParagraphFormat::character`], and that half has to land on the runs
    /// inside the paragraph rather than on the paragraph — it is what those
    /// runs were inheriting. Under their own overrides, for the same reason.
    pub fn flatten_paragraph_style(&mut self, id: ParagraphStyleId, format: &ParagraphFormat) {
        let affected: Vec<Range<usize>> = self
            .paragraphs
            .iter()
            .filter(|p| p.style == Some(id))
            .map(|p| p.range.clone())
            .collect();

        for para in &mut self.paragraphs {
            if para.style != Some(id) {
                continue;
            }
            para.local = para.local.over(format);
            para.style = None;
        }

        for range in affected {
            for run in &mut self.runs {
                if run.range.start >= range.start && run.range.end <= range.end {
                    run.local = run.local.over(&format.character);
                }
            }
        }

        self.merge_equal_neighbours();
    }

    /// The named character style every run in the range shares, and whether
    /// they all agree.
    ///
    /// `(None, true)` means every run is unstyled; `(_, false)` means they
    /// differ and the picker has nothing single to show.
    pub fn common_character_style(
        &self,
        range: Range<usize>,
    ) -> (Option<CharacterStyleId>, bool) {
        let mut runs = if range.start >= range.end {
            self.runs
                .iter()
                .rev()
                .find(|r| r.range.start < range.start)
                .or_else(|| self.runs.first())
                .into_iter()
                .collect::<Vec<_>>()
                .into_iter()
        } else {
            self.runs
                .iter()
                .filter(|r| r.range.start < range.end && range.start < r.range.end)
                .collect::<Vec<_>>()
                .into_iter()
        };
        let Some(first) = runs.next() else {
            return (None, true);
        };
        let agree = runs.all(|r| r.style == first.style);
        (first.style.filter(|_| agree), agree)
    }

    /// As above, for the paragraphs a range touches.
    pub fn common_paragraph_style(
        &self,
        range: Range<usize>,
    ) -> (Option<ParagraphStyleId>, bool) {
        if self.text.is_empty() {
            return (None, true);
        }
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len()).max(start);
        let bounds = self.paragraph_bounds(start..end);

        let mut touched = self
            .paragraphs
            .iter()
            .filter(|p| p.range.start < bounds.end && bounds.start < p.range.end);
        let Some(first) = touched.next() else {
            return (None, true);
        };
        let agree = touched.all(|p| p.style == first.style);
        (first.style.filter(|_| agree), agree)
    }

    /// Merge `format` into every paragraph the range touches.
    ///
    /// The range is first widened to whole paragraphs. `paragraphs` is a list
    /// of formatting spans and nothing stops one from covering half a
    /// paragraph, but centring half a paragraph is not a thing a typesetter
    /// can mean — so a selection that touches a paragraph formats all of it,
    /// which is also what a caret with no selection does.
    pub fn apply_paragraph_format(&mut self, range: Range<usize>, format: &ParagraphFormat) {
        if self.text.is_empty() || format == &ParagraphFormat::default() {
            return;
        }
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len()).max(start);
        let bounds = self.paragraph_bounds(start..end);

        split_span_at(&mut self.paragraphs, bounds.start, |p| &mut p.range);
        split_span_at(&mut self.paragraphs, bounds.end, |p| &mut p.range);
        // And the runs, at the same places. `resolve_run` reads the paragraph
        // holding a run's *start*, so a run straddling this boundary would
        // take this paragraph's formatting for text in the next one — styling
        // one paragraph would restyle its neighbour. Keeping runs inside
        // paragraph spans makes that impossible rather than merely unlikely.
        split_span_at(&mut self.runs, bounds.start, |r| &mut r.range);
        split_span_at(&mut self.runs, bounds.end, |r| &mut r.range);

        for para in &mut self.paragraphs {
            if para.range.start >= bounds.start && para.range.end <= bounds.end {
                para.local = format.over(&para.local);
            }
        }

        self.merge_equal_neighbours();
    }

    /// The whole paragraphs `range` touches: back to just after the previous
    /// newline, forward to just past the next one.
    ///
    /// The trailing newline is included because it belongs to the paragraph it
    /// ends, which is where its own formatting lives.
    pub fn paragraph_bounds(&self, range: Range<usize>) -> Range<usize> {
        let start = self.text[..range.start].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text[range.end..]
            .find('\n')
            .map_or(self.text.len(), |i| range.end + i + 1);
        start..end
    }

    /// A paragraph's formatting, resolved through its named style.
    ///
    /// The character half of a paragraph style is folded in by
    /// [`Story::resolve_run`] instead, because it has to sit *under* the run's
    /// own overrides and only the run knows those.
    pub fn resolve_paragraph(
        &self,
        para: &ParagraphRun,
        styles: &dyn Styles,
    ) -> ParagraphFormat {
        let mut format = ParagraphFormat::default();
        if let Some(id) = para.style
            && let Some(style) = styles.paragraph(id)
        {
            format = style.over(&format);
        }
        para.local.over(&format)
    }

    /// The alignment the whole story is set in, if its paragraphs agree.
    ///
    /// `None` when they disagree, which the shaper reads as "cannot honour
    /// this in one layout" — see the note there.
    pub fn common_alignment(&self, styles: &dyn Styles) -> Option<Alignment> {
        let mut alignments = self
            .paragraphs
            .iter()
            .map(|p| self.resolve_paragraph(p, styles).alignment.unwrap_or(Alignment::Left));
        let first = alignments.next()?;
        alignments.all(|a| a == first).then_some(first)
    }

    /// The resolved formatting every run in `range` agrees on.
    ///
    /// What the inspector shows. A field the runs disagree about comes back
    /// `None` — the same shape a field nobody has set takes, because in both
    /// cases there is no one value to show and blank is the honest answer.
    ///
    /// The values are **resolved** through the cascade rather than read off
    /// `local`, so a size box shows 12 for text nobody has sized rather than
    /// nothing at all.
    pub fn common_format(&self, range: Range<usize>, styles: &dyn Styles) -> CharacterFormat {
        if range.start >= range.end {
            // A caret, not a selection: show what typing there will produce.
            // `insert_text` joins to the run on the left, so that is the run
            // to read.
            let run = self
                .runs
                .iter()
                .rev()
                .find(|r| r.range.start < range.start)
                .or_else(|| self.runs.first());
            return match run {
                Some(run) => self.resolve_run(run, styles),
                None => styles.document_default(),
            };
        }

        let mut overlapping = self
            .runs
            .iter()
            .filter(|r| r.range.start < range.end && range.start < r.range.end);
        let Some(first) = overlapping.next() else {
            return styles.document_default();
        };
        let mut common = self.resolve_run(first, styles);
        for run in overlapping {
            let other = self.resolve_run(run, styles);
            blank_if_different(&mut common.family, other.family);
            blank_if_different(&mut common.size, other.size);
            blank_if_different(&mut common.weight, other.weight);
            blank_if_different(&mut common.italic, other.italic);
            blank_if_different(&mut common.tracking, other.tracking);
            blank_if_different(&mut common.case, other.case);
            blank_if_different(&mut common.baseline_shift, other.baseline_shift);
            blank_if_different(&mut common.line_height, other.line_height);
            blank_if_different(&mut common.colour, other.colour);
        }
        common
    }

    /// The paragraph formatting every paragraph the range touches agrees on.
    ///
    /// The range is widened the same way [`Story::apply_paragraph_format`]
    /// widens it, so what the panel shows is what the controls will change.
    pub fn common_paragraph_format(&self, range: Range<usize>) -> ParagraphFormat {
        if self.text.is_empty() {
            return ParagraphFormat::default();
        }
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len()).max(start);
        let bounds = self.paragraph_bounds(start..end);

        let mut touched = self
            .paragraphs
            .iter()
            .filter(|p| p.range.start < bounds.end && bounds.start < p.range.end);
        let Some(first) = touched.next() else {
            return ParagraphFormat::default();
        };
        let mut common = first.local.clone();
        for para in touched {
            blank_if_different(&mut common.alignment, para.local.alignment);
            blank_if_different(&mut common.indent_left, para.local.indent_left);
            blank_if_different(&mut common.indent_right, para.local.indent_right);
            blank_if_different(&mut common.indent_first, para.local.indent_first);
            blank_if_different(&mut common.space_before, para.local.space_before);
            blank_if_different(&mut common.space_after, para.local.space_after);
            blank_if_different(&mut common.hyphenate, para.local.hyphenate);
        }
        common
    }

    /// Fold together adjacent runs that would draw the same.
    ///
    /// Two runs that meet **on a paragraph boundary** are left apart even when
    /// they say the same thing, because merging them would recreate a run
    /// straddling two paragraphs — and `resolve_run` reads only the paragraph
    /// a run starts in, so the far half would take the near half's paragraph
    /// formatting. The paragraph mutators split runs at those boundaries for
    /// exactly this reason; folding them back would undo it in the same call.
    pub fn merge_equal_neighbours(&mut self) {
        let paragraph_edges: Vec<usize> = self.paragraphs.iter().map(|p| p.range.start).collect();

        let mut merged: Vec<Run> = Vec::with_capacity(self.runs.len());
        for run in std::mem::take(&mut self.runs) {
            let on_edge = paragraph_edges.contains(&run.range.start);
            match merged.last_mut() {
                Some(previous) if !on_edge && previous.same_formatting(&run) => {
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

/// Blank a shown value the spans disagree about.
fn blank_if_different<T: PartialEq>(shown: &mut Option<T>, other: Option<T>) {
    if *shown != other {
        *shown = None;
    }
}

/// Split whichever span strictly contains `offset` in two, so that `offset`
/// becomes a boundary.
///
/// "Strictly" is what makes this idempotent: a span that already starts or
/// ends at `offset` is left alone, so splitting twice costs nothing and the
/// gap-free invariant is never disturbed.
fn split_span_at<T: Clone>(
    spans: &mut Vec<T>,
    offset: usize,
    range_of: impl Fn(&mut T) -> &mut Range<usize>,
) {
    let found = spans.iter_mut().position(|span| {
        let r = range_of(span);
        r.start < offset && offset < r.end
    });
    let Some(index) = found else {
        return;
    };
    let mut tail = spans[index].clone();
    let head = range_of(&mut spans[index]);
    let end = head.end;
    head.end = offset;
    *range_of(&mut tail) = offset..end;
    spans.insert(index + 1, tail);
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

    /// A `Styles` with exactly one character style and one paragraph style.
    struct OneOfEach {
        character: CharacterFormat,
        paragraph: ParagraphFormat,
        default: CharacterFormat,
    }

    impl Styles for OneOfEach {
        fn character(&self, _: CharacterStyleId) -> Option<&CharacterFormat> {
            Some(&self.character)
        }
        fn paragraph(&self, _: ParagraphStyleId) -> Option<&ParagraphFormat> {
            Some(&self.paragraph)
        }
        fn document_default(&self) -> CharacterFormat {
            self.default.clone()
        }
    }

    fn sized(size: f32) -> CharacterFormat {
        CharacterFormat {
            size: Some(size),
            ..CharacterFormat::default()
        }
    }

    #[test]
    fn each_level_of_the_cascade_overrides_the_one_before_it() {
        let mut story = Story::new("word");
        let key = CharacterStyleId::default();
        let para_key = ParagraphStyleId::default();

        story.paragraphs[0].style = Some(para_key);
        story.paragraphs[0].local.character = sized(30.0);
        story.runs[0].style = Some(key);
        story.runs[0].local = sized(50.0);

        let styles = OneOfEach {
            character: sized(40.0),
            paragraph: ParagraphFormat {
                character: sized(20.0),
                ..ParagraphFormat::default()
            },
            default: sized(10.0),
        };

        // Every level states a size, so the last one standing must win.
        assert_eq!(story.resolve_run(&story.runs[0], &styles).size, Some(50.0));

        // Take the run's own away and the character style shows through.
        story.runs[0].local = CharacterFormat::default();
        assert_eq!(story.resolve_run(&story.runs[0], &styles).size, Some(40.0));

        // And so on down.
        story.runs[0].style = None;
        assert_eq!(story.resolve_run(&story.runs[0], &styles).size, Some(30.0));

        story.paragraphs[0].local.character = CharacterFormat::default();
        assert_eq!(story.resolve_run(&story.runs[0], &styles).size, Some(20.0));

        story.paragraphs[0].style = None;
        assert_eq!(story.resolve_run(&story.runs[0], &styles).size, Some(10.0));
    }

    #[test]
    fn a_run_naming_a_style_that_is_gone_falls_through_rather_than_failing() {
        // A deleted style should leave text looking plainer, never leave a
        // document unopenable.
        let mut story = Story::new("word");
        story.runs[0].style = Some(CharacterStyleId::default());
        story.runs[0].local = sized(18.0);

        let resolved = story.resolve_run(&story.runs[0], &NoStyles::default());
        assert_eq!(resolved.size, Some(18.0));
    }

    #[test]
    fn no_styles_resolves_local_over_the_default() {
        let mut story = Story::new("word");
        story.runs[0].local = CharacterFormat {
            weight: Some(700),
            ..CharacterFormat::default()
        };

        let resolved = story.resolve_run(&story.runs[0], &NoStyles::default());
        assert_eq!(resolved.weight, Some(700), "the run's own is kept");
        assert_eq!(resolved.size, Some(12.0), "and the rest is inherited");
    }

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
        Format(usize, usize, u16),
        FormatParagraph(usize, usize),
    }

    fn an_edit() -> impl Strategy<Value = Edit> {
        prop_oneof![
            (0usize..40, "[a-z]{0,4}").prop_map(|(at, t)| Edit::Insert(at, t)),
            (0usize..40, 0usize..40).prop_map(|(a, b)| Edit::Delete(a.min(b), a.max(b))),
            (0usize..40, 0usize..40, 100u16..900)
                .prop_map(|(a, b, w)| Edit::Format(a.min(b), a.max(b), w)),
            (0usize..40, 0usize..40)
                .prop_map(|(a, b)| Edit::FormatParagraph(a.min(b), a.max(b))),
        ]
    }


    // --- applying character formatting ---------------------------------

    #[test]
    fn formatting_the_middle_of_a_run_splits_it_into_three() {
        let mut story = Story::new("the quick brown fox");
        story.apply_character_format(4..9, &bold());

        assert!(story.runs_are_sound());
        assert_eq!(story.runs.len(), 3);
        assert_eq!(story.runs[0].range, 0..4);
        assert_eq!(story.runs[1].range, 4..9);
        assert_eq!(story.runs[2].range, 9..19);
        assert_eq!(story.runs[1].local.weight, Some(700));
        assert_eq!(story.runs[0].local.weight, None);
        assert_eq!(story.runs[2].local.weight, None);
    }

    #[test]
    fn formatting_the_whole_story_leaves_one_run() {
        let mut story = Story::new("the quick brown fox");
        story.apply_character_format(0..19, &bold());

        assert_eq!(story.runs.len(), 1, "nothing to split, nothing to fold");
        assert_eq!(story.runs[0].local.weight, Some(700));
        assert!(story.runs_are_sound());
    }

    #[test]
    fn formatting_wins_over_the_run_and_leaves_the_rest_alone() {
        // The inspector changes one control at a time. Setting bold must not
        // discard a size the run was already given.
        let mut story = Story::new("ab");
        story.runs[0].local.size = Some(9.0);
        story.runs[0].local.italic = Some(true);
        story.apply_character_format(0..2, &bold());

        let local = &story.runs[0].local;
        assert_eq!(local.weight, Some(700), "the new format wins");
        assert_eq!(local.size, Some(9.0), "and says nothing about size");
        assert_eq!(local.italic, Some(true));
    }

    #[test]
    fn applying_the_same_format_twice_changes_nothing_the_second_time() {
        let mut story = Story::new("the quick brown fox");
        story.apply_character_format(4..9, &bold());
        let once = story.runs.clone();
        story.apply_character_format(4..9, &bold());

        assert_eq!(story.runs, once, "the split is idempotent");
    }

    #[test]
    fn formatting_two_halves_the_same_way_folds_them_back_into_one() {
        let mut story = Story::new("abcd");
        story.apply_character_format(0..2, &bold());
        story.apply_character_format(2..4, &bold());

        assert_eq!(
            story.runs.len(),
            1,
            "identical neighbours must not accumulate: {:?}",
            story.runs
        );
        assert!(story.runs_are_sound());
    }

    #[test]
    fn an_empty_format_is_a_no_op_rather_than_a_reset() {
        let mut story = Story::new("abcd");
        story.apply_character_format(0..4, &bold());
        story.apply_character_format(1..3, &CharacterFormat::default());

        assert_eq!(story.runs.len(), 1);
        assert_eq!(story.runs[0].local.weight, Some(700));
    }

    #[test]
    fn an_empty_range_formats_nothing() {
        let mut story = Story::new("abcd");
        story.apply_character_format(2..2, &bold());
        assert_eq!(story.runs[0].local.weight, None);
        assert_eq!(story.runs.len(), 1);
    }

    // --- applying paragraph formatting ---------------------------------

    fn centred() -> ParagraphFormat {
        ParagraphFormat {
            alignment: Some(Alignment::Centre),
            ..ParagraphFormat::default()
        }
    }

    #[test]
    fn a_paragraph_format_widens_to_the_whole_paragraph() {
        // Centring half a paragraph is not a thing a typesetter can mean, so a
        // selection inside one formats all of it.
        let mut story = Story::new("first\nsecond\nthird");
        story.apply_paragraph_format(7..9, &centred());

        assert!(story.runs_are_sound());
        assert_eq!(story.paragraphs.len(), 3);
        assert_eq!(story.paragraphs[0].range, 0..6);
        assert_eq!(story.paragraphs[1].range, 6..13);
        assert_eq!(story.paragraphs[2].range, 13..18);
        assert_eq!(story.paragraphs[1].local.alignment, Some(Alignment::Centre));
        assert_eq!(story.paragraphs[0].local.alignment, None);
        assert_eq!(story.paragraphs[2].local.alignment, None);
    }

    #[test]
    fn a_caret_with_no_selection_formats_the_paragraph_it_sits_in() {
        let mut story = Story::new("first\nsecond");
        story.apply_paragraph_format(
            8..8,
            &ParagraphFormat {
                alignment: Some(Alignment::Right),
                ..ParagraphFormat::default()
            },
        );

        assert_eq!(story.paragraphs.len(), 2);
        assert_eq!(story.paragraphs[1].local.alignment, Some(Alignment::Right));
        assert_eq!(story.paragraphs[0].local.alignment, None);
    }

    #[test]
    fn a_selection_spanning_two_paragraphs_formats_both_whole() {
        let mut story = Story::new("one\ntwo\nthree");
        story.apply_paragraph_format(2..5, &centred());

        assert_eq!(story.paragraphs.len(), 2, "{:?}", story.paragraphs);
        assert_eq!(story.paragraphs[0].range, 0..8);
        assert_eq!(story.paragraphs[0].local.alignment, Some(Alignment::Centre));
        assert_eq!(story.paragraphs[1].range, 8..13);
        assert!(story.runs_are_sound());
    }

    #[test]
    fn character_formatting_leaves_the_paragraphs_alone() {
        // Two independent span lists over one string. Splitting runs must not
        // split paragraphs, or a bold word would take its own alignment.
        let mut story = Story::new("first\nsecond");
        story.apply_character_format(6..8, &bold());

        assert_eq!(story.runs.len(), 3);
        assert_eq!(story.paragraphs.len(), 1, "{:?}", story.paragraphs);
    }

    // --- replacing the text --------------------------------------------

    #[test]
    fn replacing_the_text_keeps_the_runs_sound() {
        // `Command::SetText` used to assign `story.text` directly, which left
        // the runs describing a length the text no longer had.
        let mut story = Story::new("the quick brown fox");
        story.apply_character_format(0..19, &bold());
        story.set_text("short");

        assert_eq!(story.text, "short");
        assert!(
            story.runs_are_sound(),
            "runs {:?} do not describe {:?}",
            story.runs,
            story.text
        );
        assert_eq!(
            story.runs[0].local.weight,
            Some(700),
            "replacing content says nothing about formatting"
        );
    }

    #[test]
    fn replacing_the_text_with_nothing_leaves_no_runs() {
        let mut story = Story::new("abc");
        story.set_text("");
        assert!(story.runs.is_empty());
        assert!(story.paragraphs.is_empty());
        assert!(story.runs_are_sound());
    }


    // --- what the inspector shows ---------------------------------------

    #[test]
    fn one_run_shows_its_resolved_formatting() {
        // Resolved, not local: a size box must read 12 for text nobody has
        // sized, rather than blank.
        let story = Story::new("abcd");
        let shown = story.common_format(0..4, &NoStyles::default());
        assert_eq!(shown.size, Some(12.0));
        assert_eq!(shown.family.as_deref(), Some("sans-serif"));
    }

    #[test]
    fn runs_that_disagree_show_nothing_for_that_field() {
        let mut story = Story::new("abcd");
        story.apply_character_format(
            0..2,
            &CharacterFormat {
                size: Some(9.0),
                ..CharacterFormat::default()
            },
        );
        let shown = story.common_format(0..4, &NoStyles::default());

        assert_eq!(shown.size, None, "9 and 12 have no common value");
        assert_eq!(
            shown.family.as_deref(),
            Some("sans-serif"),
            "but the family they agree on still shows"
        );
    }

    #[test]
    fn a_selection_inside_one_run_shows_that_runs_formatting() {
        let mut story = Story::new("the quick brown fox");
        story.apply_character_format(4..9, &bold());
        let shown = story.common_format(5..8, &NoStyles::default());
        assert_eq!(shown.weight, Some(700));
    }

    #[test]
    fn a_caret_shows_what_typing_there_will_produce() {
        // `insert_text` joins to the run on the left, so a caret just after a
        // bold word must show bold — otherwise the panel would say one thing
        // and the next keystroke do another.
        let mut story = Story::new("abcd");
        story.apply_character_format(0..2, &bold());

        assert_eq!(
            story.common_format(2..2, &NoStyles::default()).weight,
            Some(700),
            "just after the bold run"
        );
        assert_eq!(
            story.common_format(3..3, &NoStyles::default()).weight,
            None,
            "inside the plain run"
        );
    }

    #[test]
    fn a_caret_at_the_start_shows_the_first_run() {
        let mut story = Story::new("abcd");
        story.apply_character_format(0..2, &bold());
        assert_eq!(
            story.common_format(0..0, &NoStyles::default()).weight,
            Some(700)
        );
    }

    #[test]
    fn an_empty_story_shows_the_document_default() {
        let story = Story::new("");
        let shown = story.common_format(0..0, &NoStyles::default());
        assert_eq!(shown.size, Some(12.0), "an empty frame still has a size");
    }

    #[test]
    fn paragraphs_that_disagree_show_nothing_for_that_field() {
        let mut story = Story::new("one\ntwo");
        story.apply_paragraph_format(0..1, &centred());

        assert_eq!(
            story.common_paragraph_format(0..7).alignment,
            None,
            "centred and unset have no common value"
        );
        assert_eq!(
            story.common_paragraph_format(0..1).alignment,
            Some(Alignment::Centre),
            "and one paragraph on its own shows its own"
        );
    }

    #[test]
    fn what_the_panel_shows_is_what_the_controls_will_change() {
        // Both sides widen the range the same way, so a caret in the second
        // paragraph reads and writes the second paragraph.
        let mut story = Story::new("one\ntwo\nthree");
        story.apply_paragraph_format(5..5, &centred());
        assert_eq!(
            story.common_paragraph_format(5..5).alignment,
            Some(Alignment::Centre)
        );
        assert_eq!(story.common_paragraph_format(1..1).alignment, None);
    }


    // --- alignment across a story ---------------------------------------

    #[test]
    fn a_story_set_one_way_reports_that_alignment() {
        let mut story = Story::new("one\ntwo");
        story.apply_paragraph_format(0..7, &centred());
        assert_eq!(
            story.common_alignment(&NoStyles::default()),
            Some(Alignment::Centre)
        );
    }

    #[test]
    fn a_story_with_two_alignments_reports_none() {
        let mut story = Story::new("one\ntwo");
        story.apply_paragraph_format(0..1, &centred());
        assert_eq!(
            story.common_alignment(&NoStyles::default()),
            None,
            "one layout cannot be two alignments at once"
        );
    }

    #[test]
    fn a_story_nobody_has_aligned_reads_as_left() {
        let story = Story::new("one\ntwo");
        assert_eq!(
            story.common_alignment(&NoStyles::default()),
            Some(Alignment::Left),
            "unset is left, and unset everywhere still agrees"
        );
    }

    #[test]
    fn an_empty_story_has_no_alignment_to_report() {
        assert_eq!(Story::new("").common_alignment(&NoStyles::default()), None);
    }

    /// A `Styles` whose one paragraph style is right-aligned.
    fn right_aligned_style() -> OneOfEach {
        OneOfEach {
            character: CharacterFormat::default(),
            paragraph: ParagraphFormat {
                alignment: Some(Alignment::Right),
                ..ParagraphFormat::default()
            },
            default: NoStyles::default().default,
        }
    }

    #[test]
    fn a_paragraph_resolves_through_its_named_style() {
        // The whole point of a style: change it and every paragraph using it
        // follows.
        let styles = right_aligned_style();
        let mut story = Story::new("one");
        story.paragraphs[0].style = Some(ParagraphStyleId::default());

        assert_eq!(
            story
                .resolve_paragraph(&story.paragraphs[0], &styles)
                .alignment,
            Some(Alignment::Right)
        );
    }

    #[test]
    fn a_paragraphs_own_alignment_beats_its_style() {
        let styles = right_aligned_style();
        let mut story = Story::new("one");
        story.paragraphs[0].style = Some(ParagraphStyleId::default());
        story.paragraphs[0].local.alignment = Some(Alignment::Centre);

        assert_eq!(
            story
                .resolve_paragraph(&story.paragraphs[0], &styles)
                .alignment,
            Some(Alignment::Centre)
        );
    }


    // --- attaching named styles -----------------------------------------

    #[test]
    fn a_character_style_attaches_to_the_range_and_splits_the_runs() {
        let mut story = Story::new("the quick brown fox");
        let id = CharacterStyleId::default();
        story.set_character_style(4..9, Some(id));

        assert!(story.runs_are_sound());
        assert_eq!(story.runs.len(), 3);
        assert_eq!(story.runs[1].style, Some(id));
        assert_eq!(story.runs[0].style, None);
    }

    #[test]
    fn attaching_a_style_keeps_the_overrides_a_run_already_had() {
        // A style is the floor a run sits on. Applying one must not discard a
        // size somebody set by hand.
        let mut story = Story::new("abcd");
        story.apply_character_format(0..4, &sized(30.0));
        story.set_character_style(0..4, Some(CharacterStyleId::default()));

        assert_eq!(story.runs[0].local.size, Some(30.0));
    }

    #[test]
    fn detaching_a_style_leaves_the_text_where_it_was() {
        let mut story = Story::new("abcd");
        let id = CharacterStyleId::default();
        story.set_character_style(0..4, Some(id));
        story.set_character_style(0..4, None);

        assert_eq!(story.runs.len(), 1);
        assert_eq!(story.runs[0].style, None);
        assert!(story.runs_are_sound());
    }

    #[test]
    fn a_paragraph_style_takes_the_whole_paragraph() {
        let mut story = Story::new("one\ntwo\nthree");
        let id = ParagraphStyleId::default();
        // Inside the second paragraph, so only the second is styled — and it
        // is styled whole, including the newline that ends it.
        story.set_paragraph_style(5..6, Some(id));

        assert_eq!(story.paragraphs.len(), 3, "{:?}", story.paragraphs);
        assert_eq!(story.paragraphs[1].range, 4..8);
        assert_eq!(story.paragraphs[1].style, Some(id));
        assert_eq!(story.paragraphs[0].style, None);
        assert_eq!(story.paragraphs[2].style, None);
        assert!(story.runs_are_sound());
    }

    #[test]
    fn a_picker_shows_a_style_only_when_the_runs_agree() {
        let mut story = Story::new("abcd");
        let id = CharacterStyleId::default();
        story.set_character_style(0..2, Some(id));

        assert_eq!(story.common_character_style(0..2), (Some(id), true));
        assert_eq!(
            story.common_character_style(0..4),
            (None, false),
            "styled and unstyled have nothing single to show"
        );
        assert_eq!(story.common_character_style(2..4), (None, true));
    }

    #[test]
    fn a_picker_over_an_empty_story_shows_no_style_and_agrees() {
        assert_eq!(Story::new("").common_character_style(0..0), (None, true));
        assert_eq!(Story::new("").common_paragraph_style(0..0), (None, true));
    }

    #[test]
    fn a_caret_reads_the_style_typing_there_will_join() {
        let mut story = Story::new("abcd");
        let id = CharacterStyleId::default();
        story.set_character_style(0..2, Some(id));

        assert_eq!(story.common_character_style(2..2), (Some(id), true));
        assert_eq!(story.common_character_style(3..3), (None, true));
    }


    #[test]
    fn a_run_never_straddles_a_paragraph_boundary() {
        // `resolve_run` reads the paragraph a run *starts* in. A run spanning
        // two paragraphs would therefore take the first one's formatting for
        // text in the second, so styling one paragraph would restyle its
        // neighbour.
        let mut story = Story::new("one\ntwo\nthree");
        assert_eq!(story.runs.len(), 1, "one run to begin with");

        story.set_paragraph_style(0..1, Some(ParagraphStyleId::default()));

        assert!(story.runs_are_sound());
        for run in &story.runs {
            let para = story
                .paragraphs
                .iter()
                .find(|p| p.range.contains(&run.range.start))
                .expect("a run starts inside a paragraph");
            assert!(
                run.range.end <= para.range.end,
                "run {:?} leaves paragraph {:?}",
                run.range,
                para.range
            );
        }
    }

    #[test]
    fn styling_one_paragraph_leaves_the_next_one_alone() {
        let styles = right_aligned_style();
        let mut story = Story::new("one\ntwo");
        story.set_paragraph_style(0..1, Some(ParagraphStyleId::default()));

        let resolved: Vec<Option<Alignment>> = story
            .paragraphs
            .iter()
            .map(|p| story.resolve_paragraph(p, &styles).alignment)
            .collect();
        assert_eq!(resolved, vec![Some(Alignment::Right), None]);
    }


    // --- folding a style back into the text ------------------------------

    #[test]
    fn flattening_a_character_style_keeps_the_appearance() {
        let mut story = Story::new("abcd");
        let id = CharacterStyleId::default();
        story.runs[0].local.size = Some(30.0);
        story.set_character_style(0..4, Some(id));

        let style = CharacterFormat {
            weight: Some(700),
            size: Some(9.0),
            ..CharacterFormat::default()
        };
        story.flatten_character_style(id, &style);

        assert_eq!(story.runs[0].style, None, "the reference is gone");
        assert_eq!(
            story.runs[0].local.weight,
            Some(700),
            "what only the style said is kept"
        );
        assert_eq!(
            story.runs[0].local.size,
            Some(30.0),
            "and the run's own override still beats it"
        );
        assert!(story.runs_are_sound());
    }

    #[test]
    fn flattening_leaves_runs_using_a_different_style_alone() {
        // Two styles, one deleted. A `SlotMap` gives distinct ids, which is
        // what makes this testable at all.
        let mut table: slotmap::SlotMap<CharacterStyleId, CharacterStyle> =
            slotmap::SlotMap::with_key();
        let doomed = table.insert(CharacterStyle::default());
        let kept = table.insert(CharacterStyle::default());
        assert_ne!(doomed, kept);

        let mut story = Story::new("abcd");
        story.set_character_style(0..2, Some(doomed));
        story.set_character_style(2..4, Some(kept));

        story.flatten_character_style(doomed, &CharacterFormat::default());

        assert_eq!(story.runs.len(), 2, "{:?}", story.runs);
        assert_eq!(story.runs[0].style, None);
        assert_eq!(story.runs[1].style, Some(kept), "the other style survives");
    }

    #[test]
    fn flattening_a_paragraph_style_lands_its_character_half_on_the_runs() {
        // A paragraph style's character formatting is what the runs inside it
        // were inheriting, so that is where it has to land.
        let mut story = Story::new("one\ntwo");
        let id = ParagraphStyleId::default();
        story.set_paragraph_style(0..1, Some(id));

        let format = ParagraphFormat {
            alignment: Some(Alignment::Centre),
            character: CharacterFormat {
                size: Some(30.0),
                ..CharacterFormat::default()
            },
            ..ParagraphFormat::default()
        };
        story.flatten_paragraph_style(id, &format);

        assert_eq!(story.paragraphs[0].style, None);
        assert_eq!(story.paragraphs[0].local.alignment, Some(Alignment::Centre));
        assert_eq!(
            story.runs[0].local.size,
            Some(30.0),
            "the first paragraph's runs kept the size it gave them"
        );
        assert_eq!(
            story.runs.last().expect("a run").local.size,
            None,
            "and the second paragraph was never using it"
        );
        assert!(story.runs_are_sound());
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
                    Edit::Format(a, b, weight) => {
                        // Splitting runs is the operation most likely to leave
                        // a gap, so it is generated alongside the edits rather
                        // than tested on its own.
                        story.apply_character_format(
                            a.min(story.text.len())..b.min(story.text.len()),
                            &CharacterFormat {
                                weight: Some(weight),
                                ..CharacterFormat::default()
                            },
                        );
                    }
                    Edit::FormatParagraph(a, b) => {
                        story.apply_paragraph_format(
                            a.min(story.text.len())..b.min(story.text.len()),
                            &ParagraphFormat {
                                alignment: Some(Alignment::Centre),
                                ..ParagraphFormat::default()
                            },
                        );
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
