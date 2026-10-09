//! Character styles a paragraph style applies by itself: nested styles, GREP
//! styles and line styles, as InDesign's Paragraph Style Options has them.
//!
//! Each is a rule on the paragraph's format that names a character style and
//! says where it goes — up to or through the first so many of something, on
//! whatever a pattern matches, on the first so many lines. None of it is
//! stored on the text: the text keeps the runs it was given, and these are
//! laid over it when it is shaped, so editing the copy moves them with it and
//! editing the rule moves every paragraph that follows it.
//!
//! **Where they sit in the cascade.** Above the paragraph's own formatting
//! and below a character style applied by hand and the run's own overrides:
//!
//! ```text
//! paragraph style -> paragraph local -> line -> nested -> GREP
//!                 -> character style -> run local
//! ```
//!
//! so a word made italic by hand inside a bold run-in head stays italic, and
//! a GREP style wins over a nested one where both reach.

use std::borrow::Cow;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::story::{CharacterFormat, CharacterStyleId, ParagraphFormat, Run, Story, Styles};

/// A character style from the start of the paragraph, or from where the one
/// before it stopped, up to or through the `count`th `delimiter`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestedStyle {
    /// `None` is InDesign's [None]: the stretch is passed over, set as the
    /// paragraph is, and the next nested style begins after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<CharacterStyleId>,
    /// Through the delimiter, taking it in; or up to it, leaving it out.
    pub through: bool,
    pub count: u16,
    pub delimiter: Delimiter,
}

/// What a nested style counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Delimiter {
    /// Ended by `.`, `!` or `?`.
    Sentences,
    /// Ended by the space after them.
    Words,
    Characters,
    Letters,
    Digits,
    Tabs,
    /// Any one of these characters, as typed into InDesign's delimiter box.
    AnyOf(String),
}

impl Delimiter {
    /// What a person reads in the menu.
    pub fn label(&self) -> String {
        match self {
            Delimiter::Sentences => "Sentences".to_owned(),
            Delimiter::Words => "Words".to_owned(),
            Delimiter::Characters => "Characters".to_owned(),
            Delimiter::Letters => "Letters".to_owned(),
            Delimiter::Digits => "Digits".to_owned(),
            Delimiter::Tabs => "Tabs".to_owned(),
            Delimiter::AnyOf(chars) => format!("\u{201C}{chars}\u{201D}"),
        }
    }
}

/// A character style wherever `pattern` matches in the paragraph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GrepStyle {
    pub style: CharacterStyleId,
    /// A regular expression, as the `regex` crate reads one. A pattern that
    /// does not compile matches nothing: a paragraph style is not the place
    /// for a typing mistake to stop the page from drawing.
    pub pattern: String,
}

/// A character style on the next `lines` lines of the paragraph: InDesign's
/// nested line styles, which set an opening line in small capitals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LineStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<CharacterStyleId>,
    pub lines: u16,
}

/// A stretch of text and the character style laid over it.
pub type Span = (Range<usize>, CharacterStyleId);

/// Where the nested and GREP styles of every paragraph in `story` go, in the
/// order they are laid on: nested first, then GREP.
pub fn spans(story: &Story, styles: &dyn Styles) -> Vec<Span> {
    let mut nested = Vec::new();
    let mut grep = Vec::new();
    for paragraph in &story.paragraphs {
        let format = story.resolve_paragraph(paragraph, styles);
        let range = paragraph_text(story, paragraph.range.clone());
        if let Some(rules) = &format.nested {
            nested.extend(nested_spans(&story.text, range.clone(), rules));
        }
        if let Some(rules) = &format.grep {
            grep.extend(grep_spans(&story.text, range, rules));
        }
    }
    nested.extend(grep);
    nested
}

/// Whether anything in `format` lays a style on by itself, so a story with
/// none of it costs a glance.
pub fn has_rules(format: &ParagraphFormat) -> bool {
    format.nested.as_ref().is_some_and(|r| !r.is_empty())
        || format.grep.as_ref().is_some_and(|r| !r.is_empty())
        || format.line_styles.as_ref().is_some_and(|r| !r.is_empty())
}

/// The character styles the rules in `format` name, for anything keyed on
/// what a paragraph draws as.
pub fn named(format: &ParagraphFormat) -> Vec<CharacterStyleId> {
    let nested = format.nested.iter().flatten().filter_map(|r| r.style);
    let grep = format.grep.iter().flatten().map(|r| r.style);
    let lines = format.line_styles.iter().flatten().filter_map(|r| r.style);
    nested.chain(grep).chain(lines).collect()
}

/// A paragraph's range without its closing newline, which no rule reaches.
fn paragraph_text(story: &Story, range: Range<usize>) -> Range<usize> {
    let end = range.end.min(story.text.len());
    let start = range.start.min(end);
    if story.text[start..end].ends_with('\n') {
        start..end - 1
    } else {
        start..end
    }
}

/// The nested styles, one after another from the paragraph's start.
pub fn nested_spans(text: &str, paragraph: Range<usize>, rules: &[NestedStyle]) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut at = paragraph.start;
    for rule in rules {
        if at >= paragraph.end {
            break;
        }
        let (end, found) = reach(text, at..paragraph.end, rule);
        if let Some(style) = rule.style
            && end > at
        {
            spans.push((at..end, style));
        }
        at = end;
        // A delimiter that never came took the rest of the paragraph.
        if !found {
            break;
        }
    }
    spans
}

/// Where one nested style ends, from `range.start`: after the `count`th
/// delimiter through it, or at it up to it — and whether there were that
/// many, or the paragraph ended first.
fn reach(text: &str, range: Range<usize>, rule: &NestedStyle) -> (usize, bool) {
    let wanted = usize::from(rule.count.max(1));
    let mut seen = 0;
    let mut in_word = false;
    for (offset, c) in text[range.clone()].char_indices() {
        let at = range.start + offset;
        let hit = match &rule.delimiter {
            Delimiter::Sentences => matches!(c, '.' | '!' | '?'),
            Delimiter::Words => {
                // The space that ends a word: one after something that was
                // not a space.
                let ends = c.is_whitespace() && in_word;
                in_word = !c.is_whitespace();
                ends
            }
            Delimiter::Characters => true,
            Delimiter::Letters => c.is_alphabetic(),
            Delimiter::Digits => c.is_numeric(),
            Delimiter::Tabs => c == '\t',
            Delimiter::AnyOf(chars) => chars.contains(c),
        };
        if hit {
            seen += 1;
            if seen == wanted {
                return if rule.through {
                    (at + c.len_utf8(), true)
                } else {
                    (at, true)
                };
            }
        }
    }
    (range.end, false)
}

/// Every non-empty match of every GREP style in the paragraph.
pub fn grep_spans(text: &str, paragraph: Range<usize>, rules: &[GrepStyle]) -> Vec<Span> {
    let slice = &text[paragraph.clone()];
    let mut spans = Vec::new();
    for rule in rules {
        let Some(pattern) = compiled(&rule.pattern) else {
            continue;
        };
        for found in pattern.find_iter(slice) {
            if !found.is_empty() {
                spans.push((
                    paragraph.start + found.start()..paragraph.start + found.end(),
                    rule.style,
                ));
            }
        }
    }
    spans
}

/// A pattern compiled once per thread and kept: the same few patterns are
/// asked for on every paragraph of every page.
fn compiled(pattern: &str) -> Option<regex::Regex> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static SEEN: RefCell<HashMap<String, Option<regex::Regex>>> = RefCell::new(HashMap::new());
    }
    SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        if seen.len() > 256 {
            seen.clear();
        }
        seen.entry(pattern.to_owned())
            .or_insert_with(|| regex::Regex::new(pattern).ok())
            .clone()
    })
}

/// The line styles of a paragraph whose lines end at `line_ends`, stored
/// offsets after each line, in order.
pub fn line_spans(paragraph: Range<usize>, line_ends: &[usize], rules: &[LineStyle]) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut line = 0usize;
    let mut at = paragraph.start;
    for rule in rules {
        let last = line + usize::from(rule.lines.max(1));
        let end = line_ends
            .get(last - 1)
            .copied()
            .unwrap_or(paragraph.end)
            .min(paragraph.end);
        if let Some(style) = rule.style
            && end > at
        {
            spans.push((at..end, style));
        }
        if last >= line_ends.len() {
            break;
        }
        line = last;
        at = end;
    }
    spans
}

/// The line styles of a paragraph whose first `found.len()` rules are
/// known to end at `found`: those exactly, and the next rule over the whole
/// of the rest — which is how the end of *its* lines is found, since lines
/// set in the style break where the style makes them break.
pub fn line_spans_settling(
    paragraph: Range<usize>,
    rules: &[LineStyle],
    found: &[usize],
) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut at = paragraph.start;
    for (i, rule) in rules.iter().enumerate() {
        let end = match found.get(i) {
            Some(end) => (*end).min(paragraph.end),
            None if i == found.len() => paragraph.end,
            None => break,
        };
        if let Some(style) = rule.style
            && end > at
        {
            spans.push((at..end, style));
        }
        at = end;
    }
    spans
}

/// How many lines the rules up to and including the `k`th take, together.
pub fn lines_through(rules: &[LineStyle], k: usize) -> usize {
    rules
        .iter()
        .take(k + 1)
        .map(|r| usize::from(r.lines.max(1)))
        .sum()
}

/// `story` with `spans` laid over its runs, for shaping.
///
/// Each run is cut where a span begins or ends; a piece under one or more
/// spans takes them — later ones over earlier — beneath its own character
/// style and overrides, folded into the piece's local formatting so the
/// cascade reads it in the right place. Borrowed unchanged when there are no
/// spans, which is nearly always.
pub fn laid_over<'a>(story: &'a Story, styles: &dyn Styles, spans: &[Span]) -> Cow<'a, Story> {
    if spans.is_empty() {
        return Cow::Borrowed(story);
    }
    let mut cuts: Vec<usize> = spans.iter().flat_map(|(r, _)| [r.start, r.end]).collect();
    cuts.sort_unstable();
    cuts.dedup();

    let mut runs: Vec<Run> = Vec::with_capacity(story.runs.len() + cuts.len());
    for run in &story.runs {
        let mut bounds = vec![run.range.start];
        bounds.extend(
            cuts.iter()
                .copied()
                .filter(|c| *c > run.range.start && *c < run.range.end),
        );
        bounds.push(run.range.end);
        for piece in bounds.windows(2) {
            let range = piece[0]..piece[1];
            let mut automatic = CharacterFormat::default();
            for (span, style) in spans {
                if span.start <= range.start && range.end <= span.end && range.start < range.end {
                    automatic = styles.character_chain(*style).over(&automatic);
                }
            }
            if automatic == CharacterFormat::default() {
                runs.push(Run {
                    range,
                    ..run.clone()
                });
                continue;
            }
            let own = run
                .style
                .map(|id| styles.character_chain(id))
                .unwrap_or_default();
            runs.push(Run {
                range,
                style: None,
                local: run.local.over(&own.over(&automatic)),
            });
        }
    }
    let mut laid = story.clone();
    laid.runs = runs;
    Cow::Owned(laid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u32) -> CharacterStyleId {
        use slotmap::KeyData;
        CharacterStyleId::from(KeyData::from_ffi(u64::from(n) | 1 << 32))
    }

    fn nested(style: u32, through: bool, count: u16, delimiter: Delimiter) -> NestedStyle {
        NestedStyle {
            style: Some(id(style)),
            through,
            count,
            delimiter,
        }
    }

    #[test]
    fn a_run_in_head_is_bold_through_the_first_colon_and_no_further() {
        let text = "Note: the rest is plain";
        let spans = nested_spans(
            text,
            0..text.len(),
            &[nested(1, true, 1, Delimiter::AnyOf(":".into()))],
        );
        assert_eq!(spans, [(0..5, id(1))]);
        let up_to = nested_spans(
            text,
            0..text.len(),
            &[nested(1, false, 1, Delimiter::AnyOf(":".into()))],
        );
        assert_eq!(up_to, [(0..4, id(1))], "up to leaves the colon out");
    }

    #[test]
    fn nested_styles_follow_one_another_and_none_passes_over() {
        // Two words in one style, a stretch passed over to the tab, then a
        // third style for one word.
        let text = "One two three\tfour five";
        let rules = [
            nested(1, true, 2, Delimiter::Words),
            NestedStyle {
                style: None,
                through: true,
                count: 1,
                delimiter: Delimiter::Tabs,
            },
            nested(3, false, 1, Delimiter::Words),
        ];
        let spans = nested_spans(text, 0..text.len(), &rules);
        assert_eq!(spans, [(0..8, id(1)), (14..18, id(3))]);
    }

    #[test]
    fn a_delimiter_that_never_comes_takes_the_rest_and_stops() {
        let text = "no full stop here";
        let rules = [
            nested(1, true, 1, Delimiter::Sentences),
            nested(2, true, 1, Delimiter::Words),
        ];
        assert_eq!(nested_spans(text, 0..text.len(), &rules), [(0..17, id(1))]);
    }

    #[test]
    fn a_grep_style_marks_every_match_and_a_bad_pattern_none() {
        let text = "Call 555 1234 or 555 9876";
        let rules = [
            GrepStyle {
                style: id(1),
                pattern: r"\d{3} \d{4}".into(),
            },
            GrepStyle {
                style: id(2),
                pattern: "(unclosed".into(),
            },
        ];
        assert_eq!(
            grep_spans(text, 0..text.len(), &rules),
            [(5..13, id(1)), (17..25, id(1))]
        );
    }

    #[test]
    fn line_styles_take_the_lines_they_are_given() {
        let ends = [10, 20, 30, 40];
        let rules = [
            LineStyle {
                style: Some(id(1)),
                lines: 1,
            },
            LineStyle {
                style: Some(id(2)),
                lines: 2,
            },
        ];
        assert_eq!(
            line_spans(0..40, &ends, &rules),
            [(0..10, id(1)), (10..30, id(2))]
        );
    }

    #[test]
    fn laid_over_cuts_runs_and_keeps_what_was_set_by_hand_on_top() {
        use crate::story::{CharacterStyle, NoStyles};
        struct Two(Vec<(CharacterStyleId, CharacterFormat)>);
        impl Styles for Two {
            fn character(&self, id: CharacterStyleId) -> Option<&CharacterFormat> {
                self.0.iter().find(|(i, _)| *i == id).map(|(_, f)| f)
            }
            fn paragraph(&self, _: crate::story::ParagraphStyleId) -> Option<&ParagraphFormat> {
                None
            }
            fn document_default(&self) -> CharacterFormat {
                NoStyles::default().default
            }
        }
        let _ = CharacterStyle::default();
        let bold = CharacterFormat {
            weight: Some(700),
            ..Default::default()
        };
        let styles = Two(vec![(id(1), bold)]);
        let mut story = Story::new("Head: body");
        story.apply_character_format(
            0..2,
            &CharacterFormat {
                italic: Some(true),
                ..Default::default()
            },
        );
        let laid = laid_over(&story, &styles, &[(0..5, id(1))]);
        let at = |offset: usize| {
            let run = laid.run_at(offset).expect("a run");
            laid.resolve_run(run, &styles)
        };
        assert_eq!(at(0).weight, Some(700), "the nested style reaches it");
        assert_eq!(at(0).italic, Some(true), "and the italic set by hand stays");
        assert_eq!(at(3).weight, Some(700));
        assert_eq!(
            at(7).weight,
            NoStyles::default().default.weight,
            "the body is not"
        );
        assert_eq!(laid.text, story.text, "the words are untouched");
    }
}
