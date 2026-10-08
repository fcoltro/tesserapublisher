//! Finding text across every story in a document, and changing it.
//!
//! Kept apart from the window that drives it, in the same way
//! [`tessera_layout::snap`] is kept apart from the viewport: the searching is
//! arithmetic over strings and has no opinion about panels, and that is what
//! makes it something a test can hold still.
//!
//! ## Offsets, and why the obvious implementation is wrong
//!
//! A hit is a **byte range into the story's own text**, because that is what
//! every edit in `tessera_text` takes. The obvious way to search without regard
//! to case — lowercase the haystack, lowercase the needle, call `find` — hands
//! back offsets into a string that no longer exists: lowercasing changes
//! lengths (`İ` is two bytes and lowercases to three), so those offsets drift
//! past the first such character and land mid-glyph. Every match here is walked
//! over the **original** text and advances by the original character's own
//! width, so an offset is always one the story will accept.

use std::ops::Range;

use tessera_document::document::Document;
use tessera_document::ids::{FrameId, StoryId};
use tessera_document::nodes::FrameKind;

/// What to look for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub needle: String,
    /// Off by default, which is what a person means by "find the word dog".
    pub match_case: bool,
    /// A match must not have a letter or digit immediately beside it.
    pub whole_word: bool,
    /// The needle is a regular expression, as InDesign's GREP tab takes it:
    /// `^` and `$` are a paragraph's start and end, `\r` a paragraph break,
    /// and the change-to text can say `$1` for what a group caught.
    pub grep: bool,
    /// Only text set in this paragraph style.
    pub paragraph_style: Option<tessera_text::story::ParagraphStyleId>,
    /// Only text set in this character style.
    pub character_style: Option<tessera_text::story::CharacterStyleId>,
}

impl Query {
    /// An empty needle finds nothing rather than finding everywhere.
    ///
    /// The empty string is a substring of every position in every story, so
    /// "find" on an empty box would report a hit between every pair of
    /// characters in the document and "change all" would splice text into all
    /// of them. A GREP that does not compile finds nothing either; the window
    /// says why ([`pattern_error`]).
    pub fn is_runnable(&self) -> bool {
        !self.needle.is_empty() && (!self.grep || self.regex().is_some())
    }

    /// The needle as a compiled expression, when it is a GREP that compiles.
    fn regex(&self) -> Option<regex::Regex> {
        build_regex(self).ok()
    }
}

/// What InDesign writes `^t`, `^p` and the rest as, in Text mode and in
/// any change-to text: the characters nobody can type into a box.
pub const TOKENS: &[(&str, char)] = &[
    ("^t", '\t'),
    ("^p", '\n'),
    ("^m", '\u{2003}'),
    ("^>", '\u{2002}'),
    ("^<", '\u{2009}'),
    ("^|", '\u{200A}'),
    ("^s", '\u{00A0}'),
    ("^_", '\u{2014}'),
    ("^=", '\u{2013}'),
    ("^-", '\u{00AD}'),
    ("^~", '\u{2011}'),
    ("^8", '\u{2022}'),
    ("^e", '\u{2026}'),
    ("^^", '^'),
];

/// `text` with every `^` token turned into its character, left to right, so
/// `^^t` is a caret and a t rather than a caret and a tab.
pub fn expand_tokens(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    'scan: while !rest.is_empty() {
        if rest.starts_with('^') {
            for (token, c) in TOKENS {
                if let Some(after) = rest.strip_prefix(token) {
                    out.push(*c);
                    rest = after;
                    continue 'scan;
                }
            }
        }
        let c = rest.chars().next().expect("not empty");
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// The GREP as the regex crate takes it: `\r` is a paragraph break, which
/// Tessera writes `\n`; `^` and `$` mean a paragraph's ends; case and whole
/// words as the boxes say.
fn build_regex(query: &Query) -> Result<regex::Regex, regex::Error> {
    let pattern = query.needle.replace(r"\r", r"\n");
    let pattern = if query.whole_word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern
    };
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!query.match_case)
        .multi_line(true)
        .build()
}

/// Why a GREP will not run, in words, or `None` when it will.
pub fn pattern_error(query: &Query) -> Option<String> {
    if !query.grep || query.needle.is_empty() {
        return None;
    }
    build_regex(query).err().map(|e| match e {
        regex::Error::Syntax(text) => text.lines().last().unwrap_or("").trim().to_string(),
        other => other.to_string(),
    })
}

/// One occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The story the text lives in.
    pub story: StoryId,
    /// A frame showing that story — the first one, in reading order.
    ///
    /// Carried so the window can go to the hit. A threaded story is shown by
    /// several frames and the text may well be in a later one; which frame a
    /// given offset lands in is a question for the layout pass, not for a
    /// search, so this is where to *start* looking rather than a promise.
    pub frame: FrameId,
    pub cell: Option<(usize, usize)>,
    /// Byte range into the story's text.
    pub range: Range<usize>,
}

/// Every occurrence in the document, in reading order.
///
/// Reading order comes from the paint order of the frames, which is the only
/// order that means anything to a person: a search that jumps between stories
/// by slot-map key order would appear to move at random.
///
/// A story is searched **once** however many frames show it. A threaded story
/// found once per frame would offer the same occurrence three times and change
/// it three times over.
pub fn search(doc: &Document, query: &Query) -> Vec<Hit> {
    if !query.is_runnable() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (story, frame, cell) in shown_stories(doc) {
        let Some(text) = doc.story(story) else {
            continue;
        };
        for range in ranges_in(&text.text, query) {
            if !formatted_as(text, range.start, query) {
                continue;
            }
            hits.push(Hit {
                story,
                frame,
                cell,
                range,
            });
        }
    }
    hits
}

/// Whether the text at `at` is set in the styles the query asks for.
fn formatted_as(story: &tessera_text::story::Story, at: usize, query: &Query) -> bool {
    let paragraph = query
        .paragraph_style
        .is_none_or(|want| story.paragraph_run_at(at).and_then(|r| r.style) == Some(want));
    let character = query
        .character_style
        .is_none_or(|want| story.run_at(at).and_then(|r| r.style) == Some(want));
    paragraph && character
}

/// What a hit is changed to: the change-to text with its tokens, or for a
/// GREP, with `$1` and the other groups filled from what this hit caught.
pub fn replacement_for(text: &str, range: &Range<usize>, query: &Query, change: &str) -> String {
    if query.grep
        && let Some(re) = query.regex()
        && let Some(caught) = re.captures_at(text, range.start)
        && caught.get(0).is_some_and(|m| m.start() == range.start)
    {
        let mut out = String::new();
        caught.expand(change, &mut out);
        return expand_tokens(&out);
    }
    expand_tokens(change)
}

/// The edits that change every hit, each to what [`replacement_for`] makes
/// of it, back to front as [`edits_for`] orders them.
pub fn edits_for_query(
    doc: &Document,
    hits: &[Hit],
    query: &Query,
    change: &str,
) -> Vec<(StoryId, Range<usize>, String)> {
    let mut edits: Vec<_> = hits
        .iter()
        .map(|hit| {
            let text = doc.story(hit.story).map_or("", |s| s.text.as_str());
            (
                hit.story,
                hit.range.clone(),
                replacement_for(text, &hit.range, query, change),
            )
        })
        .collect();
    edits.sort_by_key(|(_, range, _)| std::cmp::Reverse(range.start));
    edits
}

/// Where each edit's new text lies once all of them are made: what a change
/// of formatting is applied to after the words are changed.
pub fn landed(edits: &[(StoryId, Range<usize>, String)]) -> Vec<(StoryId, Range<usize>)> {
    let mut ordered: Vec<_> = edits.to_vec();
    ordered.sort_by_key(|(story, range, _)| (*story, range.start));
    let mut out = Vec::new();
    let mut shift: isize = 0;
    let mut current: Option<StoryId> = None;
    for (story, range, text) in ordered {
        if current != Some(story) {
            shift = 0;
            current = Some(story);
        }
        let start = (range.start as isize + shift) as usize;
        out.push((story, start..start + text.len()));
        shift += text.len() as isize - range.len() as isize;
    }
    out
}

/// Every place text is set in a character style, in reading order, each
/// with whether any of it carries formatting of its own on top — the `+`.
///
/// A place is a stretch of the style, not a run: formatting a word inside it
/// splits one run into three, and a person who applied the style once sees
/// one place, not three.
pub fn uses_of_character_style(
    doc: &Document,
    id: tessera_text::story::CharacterStyleId,
) -> Vec<(Hit, bool)> {
    let mut uses: Vec<(Hit, bool)> = Vec::new();
    for (story, frame, cell) in shown_stories(doc) {
        let Some(text) = doc.story(story) else {
            continue;
        };
        let mut open: Option<(Range<usize>, bool)> = None;
        for run in &text.runs {
            if run.style == Some(id) && !run.range.is_empty() {
                let own = !run.local.is_empty();
                open = Some(match open {
                    Some((range, was)) if range.end == run.range.start => {
                        (range.start..run.range.end, was || own)
                    }
                    Some(done) => {
                        uses.push((hit(story, frame, cell, done.0), done.1));
                        (run.range.clone(), own)
                    }
                    None => (run.range.clone(), own),
                });
            } else if let Some((range, own)) = open.take() {
                uses.push((hit(story, frame, cell, range), own));
            }
        }
        if let Some((range, own)) = open {
            uses.push((hit(story, frame, cell, range), own));
        }
    }
    uses
}

/// Every paragraph set in a paragraph style, in reading order, each with
/// whether it carries formatting of its own on top.
///
/// Paragraphs, not runs: two neighbouring paragraphs in the same style fold
/// into one run, and counting runs would say one where a reader sees two.
/// The range stops short of the paragraph's newline, so going to it selects
/// the words and not the break after them.
pub fn uses_of_paragraph_style(
    doc: &Document,
    id: tessera_text::story::ParagraphStyleId,
) -> Vec<(Hit, bool)> {
    let mut uses = Vec::new();
    for (story, frame, cell) in shown_stories(doc) {
        let Some(text) = doc.story(story) else {
            continue;
        };
        // Both lists are in text order, so one pass over each.
        let mut runs = text.paragraphs.iter().peekable();
        for range in text.paragraph_ranges() {
            while let Some(run) = runs.peek()
                && run.range.end <= range.start
            {
                runs.next();
            }
            if let Some(run) = runs.peek()
                && run.range.start <= range.start
                && run.style == Some(id)
            {
                let end = if text.text[range.clone()].ends_with('\n') {
                    range.end - 1
                } else {
                    range.end
                };
                uses.push((
                    hit(story, frame, cell, range.start..end),
                    !run.local.is_empty(),
                ));
            }
        }
    }
    uses
}

/// Every stretch of text whose own formatting names the swatch `name`, in
/// reading order: a run's colour or its underline's, a paragraph's rules.
///
/// Its own formatting only. Text coloured by a style naming the swatch is
/// that style's use, and is counted with the style: going to every word
/// set in a coloured heading style is a tour of the headings, not of the
/// swatch.
///
/// Neighbouring runs and a paragraph's own rule fold into one stretch, as a
/// reader would point at it.
pub fn uses_of_swatch(doc: &Document, name: &str) -> Vec<Hit> {
    use tessera_document::{character_names_swatch, paragraph_names_swatch};
    let mut uses = Vec::new();
    for (story, frame, cell) in shown_stories(doc) {
        let Some(text) = doc.story(story) else {
            continue;
        };
        let trimmed = |range: &Range<usize>| {
            if text
                .text
                .get(range.clone())
                .is_some_and(|t| t.ends_with('\n'))
            {
                range.start..range.end - 1
            } else {
                range.clone()
            }
        };
        let mut ranges: Vec<Range<usize>> = text
            .runs
            .iter()
            .filter(|run| character_names_swatch(&run.local, name))
            .map(|run| run.range.clone())
            .chain(
                text.paragraphs
                    .iter()
                    .filter(|run| paragraph_names_swatch(&run.local, name))
                    .map(|run| trimmed(&run.range)),
            )
            .filter(|range| !range.is_empty())
            .collect();
        ranges.sort_by_key(|range| range.start);
        let mut open: Option<Range<usize>> = None;
        for range in ranges {
            open = Some(match open {
                Some(at) if range.start <= at.end => at.start..at.end.max(range.end),
                Some(done) => {
                    uses.push(hit(story, frame, cell, done));
                    range
                }
                None => range,
            });
        }
        if let Some(range) = open {
            uses.push(hit(story, frame, cell, range));
        }
    }
    uses
}

fn hit(story: StoryId, frame: FrameId, cell: Option<(usize, usize)>, range: Range<usize>) -> Hit {
    Hit {
        story,
        frame,
        cell,
        range,
    }
}

/// A story, the first frame showing it, and the cell it fills if a table's.
type ShownStory = (StoryId, FrameId, Option<(usize, usize)>);

/// Every story a frame shows, once each, in reading order: a text frame's,
/// and each cell's of a table. With the first frame showing it, which is
/// where going to it starts.
fn shown_stories(doc: &Document) -> Vec<ShownStory> {
    let mut shown = Vec::new();
    let mut seen: Vec<StoryId> = Vec::new();

    for frame in doc.paint_order() {
        let stories: Vec<_> = match doc.frame(frame).map(|f| &f.kind) {
            Some(FrameKind::Text { story, .. }) => vec![(*story, None)],
            Some(FrameKind::Table(table)) => (0..table.rows())
                .flat_map(|row| {
                    (0..table.columns()).filter_map(move |column| {
                        table
                            .at(row, column)?
                            .cell()
                            .map(|c| (c.story, Some((row, column))))
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        for (story, cell) in stories {
            if seen.contains(&story) {
                continue;
            }
            seen.push(story);
            shown.push((story, frame, cell));
        }
    }
    shown
}

/// Every occurrence within one string, left to right and non-overlapping.
pub fn ranges_in(haystack: &str, query: &Query) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    if !query.is_runnable() {
        return found;
    }
    if query.grep {
        // The regex crate's offsets are bytes into the haystack itself, so
        // they are offsets the story accepts. An empty match — `^` alone —
        // is skipped: changing it would splice text in at every paragraph.
        if let Some(re) = query.regex() {
            found.extend(
                re.find_iter(haystack)
                    .filter(|m| !m.is_empty())
                    .map(|m| m.range()),
            );
        }
        return found;
    }
    let expanded = Query {
        needle: expand_tokens(&query.needle),
        ..query.clone()
    };
    let query = &expanded;

    let mut at = 0usize;
    while at < haystack.len() {
        // Only ever a character boundary: `at` starts at zero and every step
        // below moves it by a whole character's width.
        let Some(len) = match_at(&haystack[at..], query) else {
            at += next_char(&haystack[at..]);
            continue;
        };
        if query.whole_word && !is_whole_word(haystack, at..at + len) {
            at += next_char(&haystack[at..]);
            continue;
        }
        found.push(at..at + len);
        // Past the match, so `aa` in `aaa` is one hit and not two overlapping
        // ones. A zero-width match cannot happen — an empty needle is refused
        // above — but stepping a whole character is what stops it looping if
        // it ever could.
        at += len.max(next_char(&haystack[at..]));
    }
    found
}

/// How many bytes of `hay` the needle matches here, if it matches at all.
///
/// Returns the length **in the haystack**, which is not the needle's length
/// when the two differ only by case and the case change resizes a character.
fn match_at(hay: &str, query: &Query) -> Option<usize> {
    let mut h = hay.chars();
    let mut consumed = 0usize;

    for want in query.needle.chars() {
        let got = h.next()?;
        let same = if query.match_case {
            got == want
        } else {
            // Per character, and compared as sequences because one character
            // can lowercase into several. The haystack still advances by the
            // character it actually held.
            got.to_lowercase().eq(want.to_lowercase())
        };
        if !same {
            return None;
        }
        consumed += got.len_utf8();
    }
    Some(consumed)
}

/// Whether nothing alphanumeric touches this range on either side.
fn is_whole_word(haystack: &str, range: Range<usize>) -> bool {
    let before = haystack[..range.start].chars().next_back();
    let after = haystack[range.end..].chars().next();
    let open = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric() && c != '_');
    open(before) && open(after)
}

/// The width of the first character, or one byte for an empty string.
fn next_char(s: &str) -> usize {
    s.chars().next().map_or(1, char::len_utf8)
}

/// The edits that change every hit, ready for one undo entry.
///
/// **Back to front.** Replacing left to right moves every later offset by the
/// difference in length, so the second edit in a story would land in the wrong
/// place and the third further out still. Applied in this order, each edit only
/// disturbs text the following ones have already passed.
pub fn edits_for(hits: &[Hit], replacement: &str) -> Vec<(StoryId, Range<usize>, String)> {
    let mut edits: Vec<_> = hits
        .iter()
        .map(|hit| (hit.story, hit.range.clone(), replacement.to_string()))
        .collect();
    edits.sort_by_key(|(_, range, _)| std::cmp::Reverse(range.start));
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(needle: &str) -> Query {
        Query {
            needle: needle.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_grep_finds_by_pattern_and_changes_with_what_it_caught() {
        let q = Query {
            needle: r"(\d+) (cats?)".into(),
            grep: true,
            ..Default::default()
        };
        let text = "1 cat, 12 cats and a dog";
        let found = ranges_in(text, &q);
        assert_eq!(found, vec![0..5, 7..14]);
        assert_eq!(replacement_for(text, &found[1], &q, "$2 x$1"), "cats x12");
        // A paragraph's start and end, and \r for its break.
        let starts = Query {
            needle: r"^\w+".into(),
            grep: true,
            ..Default::default()
        };
        assert_eq!(ranges_in("one two\nthree", &starts), vec![0..3, 8..13]);
        let breaks = Query {
            needle: r"\r".into(),
            grep: true,
            ..Default::default()
        };
        assert_eq!(ranges_in("a\nb", &breaks), vec![1..2]);
    }

    #[test]
    fn a_grep_that_does_not_compile_finds_nothing_and_says_why() {
        let q = Query {
            needle: "(unclosed".into(),
            grep: true,
            ..Default::default()
        };
        assert!(!q.is_runnable());
        assert!(ranges_in("(unclosed", &q).is_empty());
        assert!(pattern_error(&q).is_some());
    }

    #[test]
    fn tokens_stand_for_the_characters_nobody_can_type() {
        assert_eq!(expand_tokens("a^tb^pc^m"), "a\tb\nc\u{2003}");
        assert_eq!(expand_tokens("^^t"), "^t", "a doubled caret is a caret");
        assert_eq!(ranges_in("one\ttwo", &query("^t")), vec![3..4]);
    }

    #[test]
    fn changed_text_lands_where_the_edits_leave_it() {
        let story = StoryId::default();
        let edits = vec![
            (story, 10..12, "xyz".to_string()),
            (story, 0..4, "a".to_string()),
        ];
        assert_eq!(landed(&edits), vec![(story, 0..1), (story, 7..10)]);
    }

    #[test]
    fn an_empty_needle_finds_nothing() {
        // It is a substring of every position, so the honest answer to "find
        // nothing" is nothing — not a hit between every pair of characters,
        // which is what "change all" would then splice into.
        assert!(ranges_in("anything at all", &query("")).is_empty());
        assert!(!query("").is_runnable());
    }

    #[test]
    fn every_occurrence_is_found_in_order() {
        let found = ranges_in("the cat sat on the mat", &query("at"));
        assert_eq!(found, vec![5..7, 9..11, 20..22]);
    }

    #[test]
    fn matches_do_not_overlap() {
        // `aa` in `aaaa` is two hits, not three. Changing overlapping hits
        // would splice a replacement into text another hit still claims.
        assert_eq!(ranges_in("aaaa", &query("aa")), vec![0..2, 2..4]);
    }

    #[test]
    fn case_is_ignored_unless_it_is_asked_for() {
        assert_eq!(ranges_in("Dog dog DOG", &query("dog")).len(), 3);

        let exact = Query {
            needle: "dog".into(),
            match_case: true,
            whole_word: false,
            ..Default::default()
        };
        assert_eq!(ranges_in("Dog dog DOG", &exact), vec![4..7]);
    }

    #[test]
    fn an_offset_survives_a_character_that_changes_width_when_lowercased() {
        // The bug this exists to prevent: lowercase the haystack, search that,
        // and hand back the offsets. `İ` is two bytes and lowercases to three,
        // so every offset after it is wrong by one — enough to land inside a
        // character and make the edit panic or corrupt the run table.
        let hay = "İstanbul and dog";
        let found = ranges_in(hay, &query("dog"));
        assert_eq!(found.len(), 1);
        let range = found[0].clone();
        assert_eq!(&hay[range.clone()], "dog", "the offset is off by a byte");
        assert!(hay.is_char_boundary(range.start) && hay.is_char_boundary(range.end));
    }

    #[test]
    fn a_needle_can_match_a_different_number_of_bytes_than_it_has() {
        // Case-insensitively, `STRASSE` does not match `Straße` — but `ß` must
        // not desynchronise the walk either. What matters is that whatever is
        // reported slices cleanly out of the original.
        let hay = "Grüße an alle";
        let found = ranges_in(hay, &query("GRÜßE"));
        for range in &found {
            assert!(hay.is_char_boundary(range.start) && hay.is_char_boundary(range.end));
        }
    }

    #[test]
    fn a_whole_word_match_refuses_a_word_it_is_only_part_of() {
        let whole = Query {
            needle: "cat".into(),
            match_case: false,
            whole_word: true,
            ..Default::default()
        };
        assert_eq!(ranges_in("the cat in concatenate", &whole), vec![4..7]);
        // Punctuation is not part of a word, so the last one still counts.
        assert_eq!(ranges_in("a cat, and cats", &whole), vec![2..5]);
    }

    #[test]
    fn a_whole_word_match_counts_the_ends_of_the_string() {
        let whole = Query {
            needle: "cat".into(),
            match_case: false,
            whole_word: true,
            ..Default::default()
        };
        assert_eq!(ranges_in("cat", &whole), vec![0..3]);
    }

    #[test]
    fn changes_are_ordered_back_to_front() {
        // Left to right, the second edit in a story lands wherever the first
        // one's change in length pushed it. This order is what lets a caller
        // apply them one after another without recomputing anything.
        let story = StoryId::default();
        let hits = vec![
            Hit {
                story,
                frame: FrameId::default(),
                cell: None,
                range: 0..3,
            },
            Hit {
                story,
                frame: FrameId::default(),
                cell: None,
                range: 10..13,
            },
        ];
        let edits = edits_for(&hits, "longer");
        assert_eq!(edits[0].1, 10..13, "the later hit is changed first");
        assert_eq!(edits[1].1, 0..3);
    }

    #[test]
    fn a_swatch_is_found_in_the_text_formatted_with_it_and_neighbours_fold() {
        use crate::app::TesseraApp;
        use crate::command::{Command, apply};
        use tessera_color::Color;
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let frame = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id: frame,
                text: "One two three\nFour".into(),
            },
        );
        let FrameKind::Text { story, .. } = state.active().document().frames[frame].kind else {
            panic!("a text frame");
        };
        let brand = Color::Swatch {
            name: "Brand".into(),
            tint: 1.0,
        };
        let colour = |state: &mut TesseraApp, range: Range<usize>, colour: Color| {
            apply(
                state,
                Command::SetCharacterFormat {
                    story,
                    range,
                    format: tessera_text::story::CharacterFormat {
                        colour: Some(colour),
                        ..Default::default()
                    },
                },
            );
        };
        // "One" and "two" coloured separately but touching, then "Four".
        colour(&mut state, 0..4, brand.clone());
        colour(&mut state, 4..7, brand.clone());
        colour(&mut state, 14..18, brand.clone());
        // Another colour is not this swatch.
        colour(&mut state, 8..13, Color::BLACK);

        let found = uses_of_swatch(state.active().document(), "Brand");
        let ranges: Vec<_> = found.iter().map(|hit| hit.range.clone()).collect();
        assert_eq!(ranges, [0..7, 14..18]);
        assert!(
            found
                .iter()
                .all(|hit| hit.frame == frame && hit.story == story)
        );
        assert!(uses_of_swatch(state.active().document(), "Other").is_empty());
    }
}
