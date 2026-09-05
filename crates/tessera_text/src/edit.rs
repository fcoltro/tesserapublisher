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
    /// It is drawn (underlined) but is not part of the story until commit.
    ime_preedit: Option<String>,
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
        }
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

    pub fn select_all(&mut self) {
        self.cursor = TextCursor {
            anchor: 0,
            position: self.story.text.len(),
        };
    }

    /// Insert text, replacing any selection. Also commits an IME composition.
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

    pub fn insert(&mut self, text: &str) {
        self.ime_preedit = None;
        self.delete_selection();
        // Through the story, so the runs come with it. Writing to `text`
        // directly would leave them describing a string that no longer
        // exists — corruption, and its symptom appears far from here.
        self.story.insert_text(self.cursor.position, text);
        self.set_cursor(self.cursor.position + text.len());
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
