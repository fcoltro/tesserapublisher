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
}

impl Query {
    /// An empty needle finds nothing rather than finding everywhere.
    ///
    /// The empty string is a substring of every position in every story, so
    /// "find" on an empty box would report a hit between every pair of
    /// characters in the document and "change all" would splice text into all
    /// of them.
    pub fn is_runnable(&self) -> bool {
        !self.needle.is_empty()
    }
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
    let mut seen: Vec<StoryId> = Vec::new();

    for frame in doc.paint_order() {
        let Some(FrameKind::Text { story, .. }) = doc.frame(frame).map(|f| &f.kind) else {
            continue;
        };
        let story = *story;
        if seen.contains(&story) {
            continue;
        }
        seen.push(story);

        let Some(text) = doc.story(story) else {
            continue;
        };
        for range in ranges_in(&text.text, query) {
            hits.push(Hit {
                story,
                frame,
                range,
            });
        }
    }
    hits
}

/// Every occurrence within one string, left to right and non-overlapping.
pub fn ranges_in(haystack: &str, query: &Query) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    if !query.is_runnable() {
        return found;
    }

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
                range: 0..3,
            },
            Hit {
                story,
                frame: FrameId::default(),
                range: 10..13,
            },
        ];
        let edits = edits_for(&hits, "longer");
        assert_eq!(edits[0].1, 10..13, "the later hit is changed first");
        assert_eq!(edits[1].1, 0..3);
    }
}
