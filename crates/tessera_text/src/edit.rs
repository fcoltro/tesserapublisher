//! The editable text buffer.
//!
//! Cursor and selection live **here** — in persistent application state —
//! rather than inside an egui widget. An immediate-mode widget is
//! reconstructed every frame, so a cursor it owned could not survive; the UI
//! layer only reports events into this buffer, and egui's own `TextEdit`
//! state is never used for canvas text (decision D3).
//!
//! All offsets are **byte** offsets into `Story::text`, but all *movement* is
//! by grapheme cluster. Those are different things, and conflating them is
//! how an editor ends up splitting an emoji or stranding a combining accent.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::story::Story;

/// `position` is the caret; `anchor` is where the selection started. Equal
/// means there is no selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextCursor {
    pub position: usize,
    pub anchor: usize,
}

pub struct EditBuffer {
    story: Story,
    cursor: TextCursor,
    /// Text the platform's input method is composing but has not committed.
    ///
    /// Not part of the story until commit — which is what keeps text nobody has
    /// chosen yet out of undo, the autosave and the file. It is *shown* by
    /// splicing it into a copy over the selection: [`Story::with_provisional`]
    /// makes the copy, `tessera_layout::resolve::Composing` carries the request,
    /// and the underline that marks it as provisional is drawn beside the caret
    /// in `tessera_ui::view::viewport`. Read through [`EditBuffer::composing`].
    ime_preedit: Option<String>,
    /// The clause inside the composition the input method is converting now, as
    /// a byte range within `ime_preedit`.
    ///
    /// **Japanese and Chinese are converted a clause at a time.** A whole
    /// sentence is composed, then each clause in turn is offered candidates,
    /// and the platform says which one by marking a range. Without it the whole
    /// composition carries one underline and nothing on screen says which part
    /// the candidate window belongs to — which is the difference between a
    /// preview somebody can convert against and a preview they cannot.
    ///
    /// The platform reports it in *characters*; it is stored here in bytes,
    /// converted once on arrival, because everything else in this module counts
    /// bytes and two units in one struct is how off-by-one bugs are made.
    ime_active: Option<Range<usize>>,
    /// Formatting chosen at a caret, waiting for text to apply it to.
    ///
    /// Character formatting needs a range and a caret is not one. Applying it
    /// to nothing is what used to happen, and it looked exactly like a broken
    /// control: the colour picker moved and the page did not. So what the panel
    /// sets at a caret is held here and lands on the next text typed, which is
    /// InDesign's behaviour and the only reading of "make this red" a blinking
    /// caret can honour.
    pending: crate::story::CharacterFormat,
}

impl EditBuffer {
    pub fn new(story: Story) -> Self {
        Self {
            story,
            cursor: TextCursor {
                position: 0,
                anchor: 0,
            },
            ime_preedit: None,
            ime_active: None,
            pending: crate::story::CharacterFormat::default(),
        }
    }

    /// Formatting chosen at a caret and not yet applied to anything.
    pub fn pending(&self) -> &crate::story::CharacterFormat {
        &self.pending
    }

    /// Hold formatting until there is text to put it on.
    ///
    /// Merged rather than replaced, so choosing red and then bold gives red
    /// bold: the panel sets one property at a time, and a caret should
    /// accumulate them the way a selection does.
    pub fn set_pending(&mut self, format: &crate::story::CharacterFormat) {
        self.pending = format.over(&self.pending);
    }

    pub fn story(&self) -> &Story {
        &self.story
    }

    pub fn cursor(&self) -> TextCursor {
        self.cursor
    }

    /// Collapses the selection to `position`, clamped into the text.
    pub fn set_cursor(&mut self, position: usize) {
        let clamped = position.min(self.story.text.len());
        self.cursor = TextCursor {
            position: clamped,
            anchor: clamped,
        };
        // Formatting chosen for one place is not an instruction about another,
        // so moving the caret abandons it. InDesign does the same, and the
        // alternative is a colour chosen an hour ago appearing in a paragraph
        // nobody connected it to.
        self.pending = crate::story::CharacterFormat::default();
    }

    /// Move the caret to `position`, keeping the anchor where it is.
    ///
    /// What dragging through text does, and the counterpart to
    /// [`EditBuffer::set_cursor`], which collapses instead.
    pub fn extend_to(&mut self, position: usize) {
        self.cursor.position = position.min(self.story.text.len());
    }

    /// Select exactly `range`, leaving the caret at its end.
    pub fn select(&mut self, range: Range<usize>) {
        let len = self.story.text.len();
        self.cursor = TextCursor {
            anchor: range.start.min(len),
            position: range.end.min(len),
        };
    }

    pub fn selection_range(&self) -> Option<Range<usize>> {
        let (start, end) = if self.cursor.position <= self.cursor.anchor {
            (self.cursor.position, self.cursor.anchor)
        } else {
            (self.cursor.anchor, self.cursor.position)
        };
        (start != end).then_some(start..end)
    }

    /// The text the selection covers, if any.
    ///
    /// What Copy and Cut put on the system clipboard. `None` rather than an
    /// empty string when nothing is selected, so a caller can tell "nothing
    /// chosen" from "an empty choice" and leave the clipboard alone.
    pub fn selected_text(&self) -> Option<&str> {
        self.selection_range().map(|r| &self.story.text[r])
    }

    pub fn select_all(&mut self) {
        self.cursor = TextCursor {
            anchor: 0,
            position: self.story.text.len(),
        };
    }

    /// Merge character formatting into a range of the buffer's own story.
    ///
    /// The buffer owns a story the document also holds, and the two are kept
    /// in step by writing the buffer's copy over the document's on every
    /// keystroke. Formatting has to reach *both* or the next keystroke would
    /// undo it — so it arrives here rather than through a `story_mut`, which
    /// would also hand every caller a way round the run invariant.
    pub fn apply_character_format(
        &mut self,
        range: Range<usize>,
        format: &crate::story::CharacterFormat,
    ) {
        self.story.apply_character_format(range, format);
    }

    /// As above, for the paragraphs a range touches.
    pub fn apply_paragraph_format(
        &mut self,
        range: Range<usize>,
        format: &crate::story::ParagraphFormat,
    ) {
        self.story.apply_paragraph_format(range, format);
    }

    /// Attach a named character style to a range of the buffer's own story.
    pub fn set_character_style(
        &mut self,
        range: Range<usize>,
        style: Option<crate::story::CharacterStyleId>,
    ) {
        self.story.set_character_style(range, style);
    }

    /// As above, for the paragraphs a range touches.
    pub fn set_paragraph_style(
        &mut self,
        range: Range<usize>,
        style: Option<crate::story::ParagraphStyleId>,
    ) {
        self.story.set_paragraph_style(range, style);
    }

    /// Fold a deleted character style into the buffer's own story.
    pub fn flatten_character_style(
        &mut self,
        id: crate::story::CharacterStyleId,
        format: &crate::story::CharacterFormat,
    ) {
        self.story.flatten_character_style(id, format);
    }

    /// Fold a deleted paragraph style into the buffer's own story.
    pub fn flatten_paragraph_style(
        &mut self,
        id: crate::story::ParagraphStyleId,
        format: &crate::story::ParagraphFormat,
    ) {
        self.story.flatten_paragraph_style(id, format);
    }

    /// Drop the local formatting in a range of the buffer's own story.
    pub fn clear_character_overrides(&mut self, range: Range<usize>) {
        self.story.clear_character_overrides(range);
    }

    /// As above, for the paragraphs a range touches.
    pub fn clear_paragraph_overrides(&mut self, range: Range<usize>) {
        self.story.clear_paragraph_overrides(range);
    }

    /// Detach a range of the buffer's own story from a character style.
    pub fn clear_character_style_link(
        &mut self,
        range: Range<usize>,
        id: crate::story::CharacterStyleId,
        format: &crate::story::CharacterFormat,
    ) {
        self.story.clear_character_style_link(range, id, format);
    }

    /// As above, for the paragraphs a range touches.
    pub fn clear_paragraph_style_link(
        &mut self,
        range: Range<usize>,
        id: crate::story::ParagraphStyleId,
        format: &crate::story::ParagraphFormat,
    ) {
        self.story.clear_paragraph_style_link(range, id, format);
    }

    /// Insert text, replacing any selection. Also commits an IME composition.
    ///
    /// The order matters and is the whole of what a commit means: the
    /// composition is dropped, the selection it stood in for is deleted, and
    /// the text lands where the selection was — which is exactly the picture
    /// [`EditBuffer::composing`] was showing while it was being composed.
    pub fn insert(&mut self, text: &str) {
        self.set_ime_preedit(None);
        self.delete_selection();
        // Where the text lands, taken before the caret moves past it.
        let at = self.cursor.position;
        // Through the story, so the runs come with it. Writing to `text`
        // directly would leave them describing a string that no longer
        // exists — corruption, and its symptom appears far from here.
        self.story.insert_text(at, text);

        // Formatting chosen at the caret, now that there is something to put
        // it on. Taken rather than read, so it lands once: the next character
        // typed inherits it from this one by `insert_text`'s join-left rule,
        // which is the same reason typing after a bold word continues bold.
        let pending = std::mem::take(&mut self.pending);
        if !pending.is_empty() && !text.is_empty() {
            self.story
                .apply_character_format(at..at + text.len(), &pending);
        }

        // `set_cursor` clears what is pending, so the caret is moved after the
        // formatting has been applied rather than before.
        self.cursor = TextCursor {
            position: (at + text.len()).min(self.story.text.len()),
            anchor: (at + text.len()).min(self.story.text.len()),
        };
    }

    pub fn delete_backward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let Some(previous) = self.previous_grapheme(self.cursor.position) else {
            return;
        };
        self.story.delete_range(previous..self.cursor.position);
        self.set_cursor(previous);
    }

    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let Some(next) = self.next_grapheme(self.cursor.position) else {
            return;
        };
        self.story.delete_range(self.cursor.position..next);
    }

    pub fn move_left(&mut self, extend: bool) {
        let target = self.previous_grapheme(self.cursor.position).unwrap_or(0);
        self.move_to(target, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        let target = self
            .next_grapheme(self.cursor.position)
            .unwrap_or(self.story.text.len());
        self.move_to(target, extend);
    }

    pub fn set_ime_preedit(&mut self, text: Option<String>) {
        self.ime_preedit = text.filter(|t| !t.is_empty());
        if self.ime_preedit.is_none() {
            // A clause of a composition that is gone would be a range into
            // nothing, and the next composition would inherit it.
            self.ime_active = None;
        }
    }

    /// Say which clause the input method is converting.
    ///
    /// Takes the range in **characters**, as the platform reports it, and
    /// converts once. Anything that does not land on a character boundary of
    /// the current composition is dropped rather than guessed at: an underline
    /// under the wrong half of a word is worse than one under all of it.
    pub fn set_ime_clause(&mut self, chars: Option<Range<usize>>) {
        self.ime_active = chars.and_then(|chars| {
            let text = self.ime_preedit.as_deref()?;
            let byte = |at: usize| {
                if at == text.chars().count() {
                    Some(text.len())
                } else {
                    text.char_indices().nth(at).map(|(b, _)| b)
                }
            };
            let (start, end) = (byte(chars.start)?, byte(chars.end)?);
            (start < end).then_some(start..end)
        });
    }

    /// What is being composed, and what it stands in for.
    ///
    /// `None` when nothing is, so the ordinary case costs nothing. The range is
    /// the selection the composition replaces — empty at a bare caret, which
    /// needs no special case — and once spliced the composition occupies
    /// `range.start .. range.start + text.len()`.
    pub fn composing(&self) -> Option<(Range<usize>, &str)> {
        let text = self.ime_preedit.as_deref()?;
        let at = self
            .selection_range()
            .unwrap_or(self.cursor.position..self.cursor.position);
        let end = self.story.text.len();
        Some((at.start.min(end)..at.end.min(end), text))
    }

    /// The clause being converted, as a byte range within the composition.
    ///
    /// Relative to the composition, not to the story, so the caller adds
    /// wherever the composition landed. `None` when the platform did not say —
    /// which many input methods never do, and a whole-composition underline is
    /// the right answer then.
    pub fn composing_clause(&self) -> Option<Range<usize>> {
        self.ime_active.clone()
    }

    pub fn ime_preedit(&self) -> Option<&str> {
        self.ime_preedit.as_deref()
    }

    /// Returns whether anything was deleted.
    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection_range() else {
            return false;
        };
        self.story.delete_range(range.clone());
        self.set_cursor(range.start);
        true
    }

    fn move_to(&mut self, position: usize, extend: bool) {
        self.cursor.position = position;
        if !extend {
            self.cursor.anchor = position;
        }
    }

    fn previous_grapheme(&self, from: usize) -> Option<usize> {
        self.story.text[..from]
            .grapheme_indices(true)
            .next_back()
            .map(|(i, _)| i)
    }

    fn next_grapheme(&self, from: usize) -> Option<usize> {
        self.story.text[from..]
            .grapheme_indices(true)
            .next()
            .map(|(_, g)| from + g.len())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn composing_over_a_selection_previews_what_committing_will_do() {
        // **The preview and the result used to disagree.** The composition was
        // spliced at the caret while the selection stayed on screen, so
        // replacing a selected word showed the new text *beside* the old one —
        // and committing then deleted the selection and put the text where it
        // had been. A preview of a result that will not happen.
        let mut buffer = EditBuffer::new(Story::new("the quick fox"));
        buffer.select(4..9); // "quick"
        assert_eq!(buffer.selection_range(), Some(4..9));

        buffer.set_ime_preedit(Some("brown".to_string()));
        let (replacing, text) = buffer.composing().expect("a composition");
        assert_eq!(replacing, 4..9, "the composition does not stand in for it");

        let shown = buffer.story().with_provisional(replacing, text);
        assert_eq!(shown.text, "the brown fox");

        // And committing produces exactly that.
        buffer.insert("brown");
        assert_eq!(buffer.story().text, "the brown fox");
        assert_eq!(
            buffer.ime_preedit(),
            None,
            "the composition outlived commit"
        );
    }

    #[test]
    fn composing_at_a_bare_caret_replaces_nothing() {
        // The ordinary case, and it needs no special path: an empty range at
        // the caret is the same operation as a replacement.
        let mut buffer = EditBuffer::new(Story::new("nihon"));
        buffer.set_cursor(5);
        buffer.set_ime_preedit(Some("go".to_string()));
        let (replacing, text) = buffer.composing().expect("a composition");
        assert!(replacing.is_empty());
        assert_eq!(
            buffer.story().with_provisional(replacing, text).text,
            "nihongo"
        );
    }

    #[test]
    fn the_clause_being_converted_arrives_in_bytes() {
        // The platform counts characters; everything here counts bytes. Two
        // units in one struct is how off-by-one bugs are made, so it converts
        // once on arrival — and the case that matters is text where the two
        // disagree, which is every language an input method exists for.
        let mut buffer = EditBuffer::new(Story::default());
        buffer.set_ime_preedit(Some("にほんご".to_string()));
        buffer.set_ime_clause(Some(1..3));
        // Three-byte characters: characters 1..3 are bytes 3..9.
        assert_eq!(buffer.composing_clause(), Some(3..9));
    }

    #[test]
    fn a_clause_running_to_the_end_is_kept() {
        // The end index is one past the last character, which `nth` cannot
        // reach — the first version of this dropped every clause that ran to
        // the end of the composition, which is most of them.
        let mut buffer = EditBuffer::new(Story::default());
        buffer.set_ime_preedit(Some("にほん".to_string()));
        buffer.set_ime_clause(Some(0..3));
        assert_eq!(buffer.composing_clause(), Some(0..9));
    }

    #[test]
    fn a_clause_that_makes_no_sense_is_dropped_rather_than_guessed_at() {
        // An underline under the wrong half of a word is worse than one under
        // all of it.
        let mut buffer = EditBuffer::new(Story::default());
        buffer.set_ime_preedit(Some("にほん".to_string()));
        // Built rather than written as literals: `2..1` and `0..0` spelled out
        // are what clippy calls an empty range that will yield no values, and it
        // is right — these are here as bad *input*, not as things to iterate.
        let nonsense = [
            Range { start: 9, end: 12 },
            Range { start: 2, end: 1 },
            Range { start: 0, end: 0 },
        ];
        for nonsense in nonsense {
            buffer.set_ime_clause(Some(nonsense.clone()));
            assert_eq!(buffer.composing_clause(), None, "{nonsense:?} was believed");
        }
    }

    #[test]
    fn a_clause_does_not_outlive_the_composition_it_indexes() {
        // Otherwise it is a range into nothing, and the next composition
        // inherits an underline belonging to the last one.
        let mut buffer = EditBuffer::new(Story::default());
        buffer.set_ime_preedit(Some("にほん".to_string()));
        buffer.set_ime_clause(Some(0..3));
        buffer.set_ime_preedit(None);
        assert_eq!(buffer.composing_clause(), None);
    }

    use super::*;
    use crate::story::Story;

    fn buffer_at_end(text: &str) -> EditBuffer {
        let mut b = EditBuffer::new(Story::new(text));
        b.set_cursor(text.len());
        b
    }

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut b = buffer_at_end("Helo");
        b.set_cursor(3);
        b.insert("l");
        assert_eq!(b.story().text, "Hello");
        assert_eq!(b.cursor().position, 4);
    }

    #[test]
    fn backspace_removes_the_character_before_the_cursor() {
        let mut b = buffer_at_end("Hello");
        b.delete_backward();
        assert_eq!(b.story().text, "Hell");
    }

    #[test]
    fn backspace_at_the_start_does_nothing() {
        let mut b = buffer_at_end("Hello");
        b.set_cursor(0);
        b.delete_backward();
        assert_eq!(b.story().text, "Hello");
        assert_eq!(b.cursor().position, 0);
    }

    #[test]
    fn delete_forward_at_the_end_does_nothing() {
        let mut b = buffer_at_end("Hello");
        b.delete_forward();
        assert_eq!(b.story().text, "Hello");
    }

    #[test]
    fn backspace_removes_a_whole_grapheme_not_a_byte() {
        // "e" followed by COMBINING ACUTE ACCENT: three bytes at the end, one
        // visible character. Deleting a byte would leave a dangling combining
        // mark attached to the wrong letter.
        let mut b = buffer_at_end("cafe\u{0301}");
        b.delete_backward();
        assert_eq!(b.story().text, "caf", "the whole grapheme must go at once");
    }

    #[test]
    fn moving_left_crosses_a_grapheme_in_one_step() {
        let mut b = buffer_at_end("cafe\u{0301}");
        let end = b.cursor().position;
        b.move_left(false);
        // "e" (1 byte) + combining acute (2 bytes) = one grapheme, 3 bytes.
        assert_eq!(b.cursor().position, end - 3);
    }

    #[test]
    fn moving_right_crosses_a_multibyte_character_in_one_step() {
        let mut b = EditBuffer::new(Story::new("日本語"));
        b.set_cursor(0);
        b.move_right(false);
        assert_eq!(b.cursor().position, 3, "one CJK character is three bytes");
    }

    #[test]
    fn moving_past_either_end_clamps() {
        let mut b = buffer_at_end("ab");
        b.move_right(false);
        assert_eq!(b.cursor().position, 2);
        b.set_cursor(0);
        b.move_left(false);
        assert_eq!(b.cursor().position, 0);
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut b = buffer_at_end("Hello");
        b.set_cursor(0);
        b.move_right(true);
        b.move_right(true);
        b.insert("J");
        assert_eq!(b.story().text, "Jllo");
    }

    #[test]
    fn backspace_deletes_the_selection_rather_than_one_character() {
        let mut b = buffer_at_end("Hello");
        b.set_cursor(0);
        b.move_right(true);
        b.move_right(true);
        b.delete_backward();
        assert_eq!(b.story().text, "llo");
    }

    #[test]
    fn select_all_covers_the_whole_story() {
        let mut b = buffer_at_end("Hello");
        b.select_all();
        assert_eq!(b.selection_range(), Some(0..5));
    }

    #[test]
    fn a_collapsed_cursor_has_no_selection() {
        let b = buffer_at_end("Hello");
        assert_eq!(b.selection_range(), None);
    }

    #[test]
    fn selecting_backwards_yields_an_ordered_range() {
        let mut b = buffer_at_end("Hello");
        b.set_cursor(4);
        b.move_left(true);
        b.move_left(true);
        assert_eq!(b.selection_range(), Some(2..4));
    }

    #[test]
    fn an_ime_preedit_is_visible_without_entering_the_text() {
        let mut b = buffer_at_end("");
        b.set_ime_preedit(Some("に".to_string()));
        assert_eq!(b.ime_preedit(), Some("に"));
        assert_eq!(b.story().text, "", "a preedit is not committed text");
    }

    #[test]
    fn committing_an_ime_composition_inserts_it_and_clears_the_preedit() {
        let mut b = buffer_at_end("");
        b.set_ime_preedit(Some("に".to_string()));
        b.insert("日本");
        assert_eq!(b.story().text, "日本");
        assert_eq!(b.ime_preedit(), None);
    }

    #[test]
    fn an_abandoned_ime_composition_leaves_no_trace() {
        let mut b = buffer_at_end("ab");
        b.set_ime_preedit(Some("に".to_string()));
        b.set_ime_preedit(None);
        assert_eq!(b.ime_preedit(), None);
        assert_eq!(b.story().text, "ab");
    }

    #[test]
    fn setting_the_cursor_past_the_end_clamps_rather_than_panicking() {
        let mut b = buffer_at_end("ab");
        b.set_cursor(999);
        assert_eq!(b.cursor().position, 2);
    }
}

#[cfg(test)]
mod run_integrity {
    use super::*;
    use crate::story::{CharacterFormat, Run};

    fn bold() -> CharacterFormat {
        CharacterFormat {
            weight: Some(700),
            ..CharacterFormat::default()
        }
    }

    /// A story reading "ab", the first character bold.
    fn two_runs() -> Story {
        let mut story = Story::new("ab");
        story.runs = vec![
            Run {
                range: 0..1,
                style: None,
                local: bold(),
            },
            Run::plain(1..2),
        ];
        story
    }

    #[test]
    fn typing_keeps_the_runs_describing_the_text() {
        let mut buffer = EditBuffer::new(two_runs());
        buffer.set_cursor(1);
        buffer.insert("XYZ");

        let story = buffer.story();
        assert_eq!(story.text, "aXYZb");
        assert!(
            story.runs_are_sound(),
            "runs {:?} no longer describe {:?}",
            story.runs,
            story.text
        );
    }

    #[test]
    fn typing_after_a_bold_character_continues_bold() {
        let mut buffer = EditBuffer::new(two_runs());
        buffer.set_cursor(1);
        buffer.insert("X");

        let story = buffer.story();
        assert_eq!(
            story.run_at(1).map(|r| r.local.clone()),
            Some(bold()),
            "the new character took the run to its left"
        );
    }

    #[test]
    fn backspacing_across_a_run_boundary_keeps_the_runs_sound() {
        let mut buffer = EditBuffer::new(two_runs());
        buffer.set_cursor(2);
        buffer.delete_backward();
        buffer.delete_backward();

        let story = buffer.story();
        assert_eq!(story.text, "");
        assert!(story.runs_are_sound());
        assert!(story.runs.is_empty());
    }

    #[test]
    fn deleting_forward_keeps_the_runs_sound() {
        let mut buffer = EditBuffer::new(two_runs());
        buffer.set_cursor(0);
        buffer.delete_forward();

        let story = buffer.story();
        assert_eq!(story.text, "b");
        assert!(story.runs_are_sound());
    }

    #[test]
    fn deleting_a_selection_keeps_the_runs_sound() {
        let mut buffer = EditBuffer::new(Story::new("hello world"));
        buffer.set_cursor(0);
        buffer.move_right(true);
        buffer.move_right(true);
        buffer.move_right(true);
        buffer.insert("X");

        let story = buffer.story();
        assert_eq!(story.text, "Xlo world");
        assert!(story.runs_are_sound());
    }
}
