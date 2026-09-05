//! Where the caret is, and which character a click landed on.
//!
//! [`crate::edit::EditBuffer`] knows the cursor as a byte offset; the screen
//! needs a rectangle. Turning one into the other is layout's job, not the
//! buffer's, so it lives with the [`Shaper`] that did the layout in the first
//! place — and it asks parley rather than re-deriving positions from
//! [`crate::shape::PositionedGlyph`], which carries no offsets and could not
//! answer for bidi text even if it did.
//!
//! Every coordinate here is **frame-local** points: the origin is the text
//! frame's top-left, exactly as [`crate::shape::ShapedText`] uses.

use crate::edit::TextCursor;
use crate::shape::Shaper;
use crate::story::{Story, Styles};

/// A rectangle in frame-local points.
///
/// Deliberately not parley's `BoundingBox`: the interface layer should not
/// have to depend on the shaping library to draw a caret.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextRect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl TextRect {
    pub fn width(self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(self) -> f64 {
        self.y1 - self.y0
    }
}

impl From<parley::BoundingBox> for TextRect {
    fn from(b: parley::BoundingBox) -> Self {
        Self {
            x0: b.x0,
            y0: b.y0,
            x1: b.x1,
            y1: b.y1,
        }
    }
}

/// What to draw for a live cursor.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CaretGeometry {
    /// The blinking bar. `None` only when there is no layout at all.
    pub caret: Option<TextRect>,
    /// One rectangle per line the selection covers. Empty when nothing is
    /// selected.
    pub selection: Vec<TextRect>,
}

impl Shaper {
    /// Where to draw the caret and the selection for `cursor`.
    ///
    /// The text is laid out again rather than cached. That happens once per
    /// frame and only while a caret is live, and it is what guarantees the
    /// caret agrees with the glyphs: one layout, asked twice.
    pub fn caret_geometry(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        cursor: TextCursor,
        caret_width: f32,
    ) -> CaretGeometry {
        let layout = self.layout(story, styles, width);

        let caret = parley::Cursor::from_byte_index(
            &layout,
            cursor.position.min(story.text.len()),
            parley::Affinity::Downstream,
        );

        let selection = if cursor.position == cursor.anchor {
            Vec::new()
        } else {
            let anchor = parley::Cursor::from_byte_index(
                &layout,
                cursor.anchor.min(story.text.len()),
                parley::Affinity::Downstream,
            );
            parley::Selection::new(anchor, caret)
                .geometry(&layout)
                .into_iter()
                .map(|(rect, _line)| rect.into())
                .collect()
        };

        CaretGeometry {
            caret: Some(caret.geometry(&layout, caret_width).into()),
            selection,
        }
    }

    /// The byte offset a click at frame-local `(x, y)` lands on.
    ///
    /// Clamped into the text by parley, so a click past the last line lands at
    /// the end rather than out of bounds.
    pub fn offset_at(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        x: f64,
        y: f64,
    ) -> usize {
        let layout = self.layout(story, styles, width);
        parley::Cursor::from_point(&layout, x as f32, y as f32).index()
    }

    /// The byte range of the word under frame-local `(x, y)`.
    ///
    /// What a double-click selects.
    pub fn word_at(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        x: f64,
        y: f64,
    ) -> std::ops::Range<usize> {
        let layout = self.layout(story, styles, width);
        parley::Selection::word_from_point(&layout, x as f32, y as f32).text_range()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::NoStyles;

    const WIDTH: f64 = 400.0;

    fn cursor(position: usize) -> TextCursor {
        TextCursor {
            position,
            anchor: position,
        }
    }

    fn caret_of(shaper: &mut Shaper, story: &Story, position: usize) -> TextRect {
        shaper
            .caret_geometry(story, &NoStyles::default(), WIDTH, cursor(position), 1.5)
            .caret
            .expect("a caret")
    }

    #[test]
    fn the_caret_advances_along_the_line_as_the_cursor_does() {
        // The bug this pins: the caret was drawn at the frame's left edge
        // whatever the cursor's offset, so it never moved as you typed.
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world");

        let start = caret_of(&mut shaper, &story, 0);
        let middle = caret_of(&mut shaper, &story, 5);
        let end = caret_of(&mut shaper, &story, 11);

        assert!(middle.x0 > start.x0, "{} !> {}", middle.x0, start.x0);
        assert!(end.x0 > middle.x0, "{} !> {}", end.x0, middle.x0);
    }

    #[test]
    fn the_caret_is_a_line_tall_and_not_a_frame_tall() {
        // It was drawn from the top of the frame to the bottom, which is
        // wrong for one line and absurd for twenty.
        let mut shaper = Shaper::new();
        let story = Story::new("one");
        let caret = caret_of(&mut shaper, &story, 0);

        // The default floor: 12pt at 1.2, as `NoStyles` states it.
        let line_height = 12.0 * 1.2;
        assert!(
            caret.height() > 0.0 && caret.height() < line_height * 2.0,
            "caret was {} tall, a line is about {line_height}",
            caret.height()
        );
    }

    #[test]
    fn the_caret_moves_down_a_line_when_the_cursor_does() {
        let mut shaper = Shaper::new();
        let story = Story::new("first\nsecond");
        let first = caret_of(&mut shaper, &story, 0);
        let second = caret_of(&mut shaper, &story, 6);
        assert!(
            second.y0 > first.y0,
            "second line should sit lower: {} vs {}",
            second.y0,
            first.y0
        );
    }

    #[test]
    fn clicking_across_the_line_walks_the_offset_forward() {
        // What makes placing a cursor by clicking possible at all.
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world");
        let y = 12.0 / 2.0;

        let left = shaper.offset_at(&story, &NoStyles::default(), WIDTH, 0.0, y);
        let right = shaper.offset_at(&story, &NoStyles::default(), WIDTH, 1000.0, y);
        assert_eq!(left, 0, "a click at the far left is the start");
        assert_eq!(right, story.text.len(), "and past the end is the end");
    }

    #[test]
    fn a_click_lands_back_on_the_offset_whose_caret_it_is() {
        // Round trip: the two directions have to agree, or clicking a caret
        // would move it.
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world");
        for offset in [0, 3, 7, 11] {
            let caret = caret_of(&mut shaper, &story, offset);
            let mid_y = (caret.y0 + caret.y1) / 2.0;
            // Just right of the caret, which is the character it precedes.
            let back = shaper.offset_at(&story, &NoStyles::default(), WIDTH, caret.x0 + 0.1, mid_y);
            assert_eq!(back, offset, "round trip failed at {offset}");
        }
    }

    #[test]
    fn a_selection_produces_a_rectangle_and_no_selection_produces_none() {
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world");

        let none = shaper.caret_geometry(&story, &NoStyles::default(), WIDTH, cursor(4), 1.5);
        assert!(none.selection.is_empty());

        let some = shaper.caret_geometry(
            &story,
            &NoStyles::default(),
            WIDTH,
            TextCursor {
                anchor: 0,
                position: 5,
            },
            1.5,
        );
        assert_eq!(some.selection.len(), 1, "one line, one rectangle");
        assert!(some.selection[0].width() > 0.0);
    }

    #[test]
    fn a_selection_spanning_two_lines_produces_two_rectangles() {
        let mut shaper = Shaper::new();
        let story = Story::new("first\nsecond");
        let both = shaper.caret_geometry(
            &story,
            &NoStyles::default(),
            WIDTH,
            TextCursor {
                anchor: 0,
                position: story.text.len(),
            },
            1.5,
        );
        assert_eq!(both.selection.len(), 2);
    }

    #[test]
    fn an_offset_past_the_end_is_clamped_rather_than_panicking() {
        // The buffer clamps too, but a stale cursor from an undo must not be
        // able to take the layout down with it.
        let mut shaper = Shaper::new();
        let story = Story::new("hi");
        let caret = caret_of(&mut shaper, &story, 9_999);
        assert!(caret.x0.is_finite());
    }

    #[test]
    fn empty_text_still_has_a_caret_to_put_the_cursor_on() {
        let mut shaper = Shaper::new();
        let caret = caret_of(&mut shaper, &Story::new(""), 0);
        assert!(caret.height() > 0.0, "an empty frame still shows a caret");
    }

    #[test]
    fn a_double_click_selects_the_word_it_landed_in() {
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world");
        let y = 12.0 / 2.0;
        let caret = caret_of(&mut shaper, &story, 8);
        let word = shaper.word_at(&story, &NoStyles::default(), WIDTH, caret.x0, y);
        assert_eq!(&story.text[word], "world");
    }
}
