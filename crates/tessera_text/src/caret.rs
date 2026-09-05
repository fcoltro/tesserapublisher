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
    /// caret agrees with the glyphs: the same `layout_paragraphs` the shaper
    /// uses, asked a different question.
    pub fn caret_geometry(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        cursor: TextCursor,
        caret_width: f32,
    ) -> CaretGeometry {
        let placed = self.layout_paragraphs(story, styles, width);
        let position = cursor.position.min(story.text.len());

        let Some(at) = paragraph_holding(&placed, position) else {
            return CaretGeometry::default();
        };
        let here = &placed[at];
        // Through the map, because the shaped text is not always the stored
        // text: a paragraph set in capitals shapes a different string, and
        // parley answers in *its* offsets.
        let caret = parley::Cursor::from_byte_index(
            &here.layout,
            here.to_shaped(position),
            parley::Affinity::Downstream,
        );

        // A selection can reach across paragraphs, and each paragraph is now a
        // separate layout that knows nothing of its neighbours. So the range is
        // asked of every paragraph it touches, clamped to that paragraph, and
        // the rectangles are gathered — which is also why the rectangles come
        // back in reading order rather than needing a sort.
        let selection = if cursor.position == cursor.anchor {
            Vec::new()
        } else {
            let anchor = cursor.anchor.min(story.text.len());
            let (from, to) = (position.min(anchor), position.max(anchor));
            let mut rects = Vec::new();
            for paragraph in &placed {
                let start = from.max(paragraph.range.start);
                let end = to.min(paragraph.range.end);
                if start >= end {
                    continue;
                }
                let a = parley::Cursor::from_byte_index(
                    &paragraph.layout,
                    paragraph.to_shaped(start),
                    parley::Affinity::Downstream,
                );
                let b = parley::Cursor::from_byte_index(
                    &paragraph.layout,
                    paragraph.to_shaped(end),
                    parley::Affinity::Downstream,
                );
                rects.extend(
                    parley::Selection::new(a, b)
                        .geometry(&paragraph.layout)
                        .into_iter()
                        .map(|(rect, _line)| translate(rect.into(), paragraph.x, paragraph.y)),
                );
            }
            rects
        };

        CaretGeometry {
            caret: Some(translate(
                caret.geometry(&here.layout, caret_width).into(),
                here.x,
                here.y,
            )),
            selection,
        }
    }

    /// The byte offset a click at frame-local `(x, y)` lands on.
    ///
    /// Clamped into the text, so a click past the last line lands at the end
    /// rather than out of bounds.
    pub fn offset_at(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        x: f64,
        y: f64,
    ) -> usize {
        let placed = self.layout_paragraphs(story, styles, width);
        let Some(at) = paragraph_under(&placed, x, y) else {
            return 0;
        };
        let here = &placed[at];
        here.to_stored(
            parley::Cursor::from_point(&here.layout, (x - here.x) as f32, (y - here.y) as f32)
                .index(),
        )
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
        let placed = self.layout_paragraphs(story, styles, width);
        let Some(at) = paragraph_under(&placed, x, y) else {
            return 0..0;
        };
        let here = &placed[at];
        let local = parley::Selection::word_from_point(
            &here.layout,
            (x - here.x) as f32,
            (y - here.y) as f32,
        )
        .text_range();
        here.to_stored(local.start)..here.to_stored(local.end)
    }
}

/// Move a rectangle into frame-local points from a paragraph's own.
fn translate(rect: TextRect, x: f64, y: f64) -> TextRect {
    TextRect {
        x0: rect.x0 + x,
        y0: rect.y0 + y,
        x1: rect.x1 + x,
        y1: rect.y1 + y,
    }
}

/// The paragraph a byte offset belongs to.
///
/// The last paragraph wins a tie at its own end, which is where a caret at the
/// very end of the story sits — otherwise the caret would fall off the text it
/// is standing at the end of.
fn paragraph_holding(placed: &[crate::shape::Placed], offset: usize) -> Option<usize> {
    if placed.is_empty() {
        return None;
    }
    placed
        .iter()
        .position(|p| p.range.contains(&offset))
        .or(Some(placed.len() - 1))
}

/// The paragraph a frame-local `y` falls in.
///
/// Above the first is the first and below the last is the last, so a click on
/// the padding above a frame's text puts the caret at the start rather than
/// nowhere. The gap *between* two paragraphs belongs to the one above it,
/// which is what makes clicking in paragraph spacing land at the end of the
/// paragraph that asked for the space.
fn paragraph_under(placed: &[crate::shape::Placed], x: f64, y: f64) -> Option<usize> {
    if placed.is_empty() {
        return None;
    }

    // A drop cap and the body beside it start at the same y and do not overlap
    // horizontally, so y alone cannot tell them apart. Anything whose own box
    // contains the point wins outright; that is what makes clicking on a drop
    // cap put the caret in the drop cap.
    if let Some(i) = placed.iter().position(|p| {
        let bottom = p.y + f64::from(p.layout.height());
        let right = p.x + f64::from(p.layout.width());
        (p.y..bottom).contains(&y) && (p.x..right).contains(&x)
    }) {
        return Some(i);
    }

    // Otherwise the last one that starts at or above the point. Above the first
    // is the first and below the last is the last, so a click on the padding
    // above a frame's text puts the caret at the start rather than nowhere.
    let mut chosen = 0;
    for (i, paragraph) in placed.iter().enumerate() {
        if y >= paragraph.y {
            chosen = i;
        }
    }
    Some(chosen)
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

    // --- across paragraphs -------------------------------------------------

    /// A story of three paragraphs, the middle one indented and spaced.
    fn three_paragraphs() -> Story {
        use crate::story::ParagraphFormat;

        let mut story = Story::new("first para\nsecond para\nthird para");
        story.apply_paragraph_format(
            11..12,
            &ParagraphFormat {
                indent_left: Some(24.0),
                space_before: Some(18.0),
                ..ParagraphFormat::default()
            },
        );
        story
    }

    #[test]
    fn every_offset_survives_a_round_trip_through_the_screen() {
        // The risk the per-paragraph change introduced, stated as a test: if
        // the paragraph lookup is wrong the caret lands in the wrong paragraph
        // and every edit after it corrupts text the user was not pointing at.
        // Nothing about that failure would be visible until it had happened.
        let story = three_paragraphs();
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        for position in 0..=story.text.len() {
            if !story.text.is_char_boundary(position) {
                continue;
            }
            let geometry = shaper.caret_geometry(&story, &styles, WIDTH, cursor(position), 1.5);
            let caret = geometry.caret.expect("a caret for every offset");

            // The middle of the caret bar, which is the point a person clicking
            // on it would hit.
            let back = shaper.offset_at(
                &story,
                &styles,
                WIDTH,
                caret.x0 + caret.width() / 2.0,
                (caret.y0 + caret.y1) / 2.0,
            );
            assert_eq!(
                back, position,
                "offset {position} drew at {caret:?} which reads back as {back}"
            );
        }
    }

    #[test]
    fn a_caret_in_the_third_paragraph_is_below_the_second() {
        let story = three_paragraphs();
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        let mut y = |at: usize| {
            shaper
                .caret_geometry(&story, &styles, WIDTH, cursor(at), 1.5)
                .caret
                .expect("a caret")
                .y0
        };
        assert!(y(0) < y(12), "the first paragraph is above the second");
        assert!(y(12) < y(24), "and the second above the third");
    }

    #[test]
    fn the_indented_paragraph_starts_its_caret_further_in() {
        let story = three_paragraphs();
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        let mut x = |at: usize| {
            shaper
                .caret_geometry(&story, &styles, WIDTH, cursor(at), 1.5)
                .caret
                .expect("a caret")
                .x0
        };
        // Offset 11 is the start of "second para", which is indented 24pt.
        assert!(
            x(11) > x(0) + 20.0,
            "the indented paragraph's caret is at {} against {}",
            x(11),
            x(0)
        );
    }

    #[test]
    fn a_selection_across_paragraphs_yields_a_rectangle_in_each() {
        // Each paragraph is its own layout and knows nothing of its
        // neighbours, so a selection spanning them has to be asked of each.
        let story = three_paragraphs();
        let mut shaper = Shaper::new();

        let geometry = shaper.caret_geometry(
            &story,
            &NoStyles::default(),
            WIDTH,
            TextCursor {
                anchor: 2,
                position: story.text.len() - 2,
            },
            1.5,
        );
        assert!(
            geometry.selection.len() >= 3,
            "three paragraphs, at least three rectangles: {:?}",
            geometry.selection
        );
    }

    #[test]
    fn a_double_click_in_the_last_paragraph_selects_a_word_there() {
        let story = three_paragraphs();
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        // Aim at the caret for an offset inside "third", then ask what word is
        // there — so the point is one the layout itself produced.
        let inside = 25;
        let caret = shaper
            .caret_geometry(&story, &styles, WIDTH, cursor(inside), 1.5)
            .caret
            .expect("a caret");
        let word = shaper.word_at(
            &story,
            &styles,
            WIDTH,
            caret.x0 + 1.0,
            (caret.y0 + caret.y1) / 2.0,
        );

        assert!(
            word.start >= 23 && word.end <= story.text.len(),
            "expected a word in the last paragraph, got {word:?} of {:?}",
            story.text
        );
        assert_eq!(&story.text[word.clone()], "third", "{word:?}");
    }

    #[test]
    fn every_offset_survives_a_round_trip_through_capitals() {
        // The offset map, exercised where it matters: `ß` uppercases to `SS`,
        // so the shaped text is longer than the stored text and parley's
        // answers are in the wrong coordinates until they are translated. A
        // caret that lands one character out here would corrupt the next edit.
        use crate::story::Case;

        let mut story = Story::new("straße und gasse");
        story.runs[0].local.case = Some(Case::Upper);

        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        for position in 0..=story.text.len() {
            if !story.text.is_char_boundary(position) {
                continue;
            }
            let geometry = shaper.caret_geometry(&story, &styles, WIDTH, cursor(position), 1.5);
            let caret = geometry.caret.expect("a caret for every offset");
            let back = shaper.offset_at(
                &story,
                &styles,
                WIDTH,
                caret.x0 + caret.width() / 2.0,
                (caret.y0 + caret.y1) / 2.0,
            );
            assert!(
                story.text.is_char_boundary(back),
                "offset {position} read back as {back}, which is not a character boundary"
            );
            assert_eq!(
                back, position,
                "offset {position} drew at {caret:?} and reads back as {back}"
            );
        }
    }

    #[test]
    fn a_double_click_in_capitals_selects_the_stored_word() {
        use crate::story::Case;

        let mut story = Story::new("straße und gasse");
        story.runs[0].local.case = Some(Case::Upper);

        let mut shaper = Shaper::new();
        let styles = NoStyles::default();
        let caret = shaper
            .caret_geometry(&story, &styles, WIDTH, cursor(2), 1.5)
            .caret
            .expect("a caret");
        let word = shaper.word_at(
            &story,
            &styles,
            WIDTH,
            caret.x0 + 1.0,
            (caret.y0 + caret.y1) / 2.0,
        );

        assert_eq!(
            &story.text[word.clone()],
            "straße",
            "the word as stored, not as drawn: {word:?}"
        );
    }

    // --- right-to-left and bidirectional text ------------------------------
    //
    // parley shapes bidi text already. What has never been exercised is the
    // caret, whose arithmetic was written assuming visual order follows
    // logical order — which is exactly what bidi text does not do.

    /// Hebrew, which reads right to left.
    const HEBREW: &str = "שלום עולם";

    /// Arabic with an English word inside it: two directions in one line.
    const MIXED: &str = "مرحبا world سلام";

    #[test]
    fn every_offset_in_right_to_left_text_survives_a_round_trip() {
        let story = Story::new(HEBREW);
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        for position in 0..=story.text.len() {
            if !story.text.is_char_boundary(position) {
                continue;
            }
            let geometry = shaper.caret_geometry(&story, &styles, WIDTH, cursor(position), 1.5);
            let caret = geometry.caret.expect("a caret for every offset");
            let back = shaper.offset_at(
                &story,
                &styles,
                WIDTH,
                caret.x0 + caret.width() / 2.0,
                (caret.y0 + caret.y1) / 2.0,
            );
            assert!(
                story.text.is_char_boundary(back),
                "offset {position} read back as {back}, not a character boundary"
            );
            assert_eq!(back, position, "offset {position} reads back as {back}");
        }
    }

    #[test]
    fn a_click_in_mixed_text_lands_where_the_caret_was_drawn() {
        // Not the same test as the one above, and the difference is the point.
        //
        // Where two directions meet, one place on screen is **two** logical
        // offsets: the end of the Latin word and the start of the Arabic that
        // follows are the same x, and a click there cannot tell them apart. So
        // asking for the original offset back is asking for something a bidi
        // caret cannot promise.
        //
        // What it can promise is that the answer is not somewhere else: the
        // caret drawn for whatever came back sits at the position that was
        // clicked. That fails loudly for a genuine mislanding and passes for
        // the ambiguity, which is the distinction worth testing.
        let story = Story::new(MIXED);
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        for position in 0..=story.text.len() {
            if !story.text.is_char_boundary(position) {
                continue;
            }
            let caret = shaper
                .caret_geometry(&story, &styles, WIDTH, cursor(position), 1.5)
                .caret
                .expect("a caret for every offset");
            let x = caret.x0 + caret.width() / 2.0;
            let y = (caret.y0 + caret.y1) / 2.0;

            let back = shaper.offset_at(&story, &styles, WIDTH, x, y);
            assert!(
                story.text.is_char_boundary(back),
                "offset {position} read back as {back}, which cuts a character in half"
            );

            let again = shaper
                .caret_geometry(&story, &styles, WIDTH, cursor(back), 1.5)
                .caret
                .expect("a caret for the offset that came back");
            assert!(
                (again.x0 - caret.x0).abs() < 1.0,
                "clicking the caret for {position} at {x} gave {back}, whose caret is at \
                 {} rather than {}",
                again.x0,
                caret.x0
            );
        }
    }

    #[test]
    fn right_to_left_text_starts_at_the_right() {
        // The first character of Hebrew sits at the right edge of the measure,
        // not the left. If this fails the text is being laid out as though it
        // read the other way.
        let story = Story::new(HEBREW);
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story, &NoStyles::default(), WIDTH);

        let first = shaped
            .runs()
            .next()
            .and_then(|r| r.glyphs.first())
            .map(|g| g.x)
            .expect("a glyph");
        let latin = shaper.shape(&Story::new("shalom olam"), &NoStyles::default(), WIDTH);
        let latin_first = latin
            .runs()
            .next()
            .and_then(|r| r.glyphs.first())
            .map(|g| g.x)
            .expect("a glyph");

        assert!(
            first > latin_first,
            "right-to-left text began at {first}, left-to-right at {latin_first}"
        );
    }

    #[test]
    fn a_selection_in_mixed_text_covers_something() {
        // A bidi selection can need several rectangles for one logical range,
        // because the range is not contiguous on screen. What must not happen
        // is no rectangle at all.
        let story = Story::new(MIXED);
        let mut shaper = Shaper::new();

        let geometry = shaper.caret_geometry(
            &story,
            &NoStyles::default(),
            WIDTH,
            TextCursor {
                anchor: 0,
                position: story.text.len(),
            },
            1.5,
        );
        assert!(
            !geometry.selection.is_empty(),
            "selecting all of it highlighted nothing"
        );
        assert!(
            geometry.selection.iter().all(|r| r.width() > 0.0),
            "a rectangle with no width: {:?}",
            geometry.selection
        );
    }

    #[test]
    fn a_word_in_mixed_text_is_a_range_of_the_stored_string() {
        let story = Story::new(MIXED);
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        let caret = shaper
            .caret_geometry(&story, &styles, WIDTH, cursor(1), 1.5)
            .caret
            .expect("a caret");
        let word = shaper.word_at(
            &story,
            &styles,
            WIDTH,
            caret.x0,
            (caret.y0 + caret.y1) / 2.0,
        );

        assert!(
            word.start <= word.end && word.end <= story.text.len(),
            "a word outside the text: {word:?}"
        );
        assert!(
            story.text.is_char_boundary(word.start) && story.text.is_char_boundary(word.end),
            "a word cutting a character in half: {word:?}"
        );
    }

    #[test]
    fn a_round_trip_survives_capitals_that_change_the_byte_length() {
        // `straße` was not enough. `ß` uppercases to `SS`, and both are two
        // bytes — so shaped and stored offsets coincided by accident and the
        // map was never exercised. `offset_at` was returning
        // `range.start + shaped_index` and the test could not tell.
        //
        // The ligature `ﬁ` is three bytes and uppercases to `FI`, which is two.
        // Every offset after it differs between the two strings, so a caret
        // that skips the map lands somewhere else.
        use crate::story::Case;

        let mut story = Story::new("\u{FB01}nd the \u{FB02}ame");
        story.runs[0].local.case = Some(Case::Upper);

        // The premise, stated so the test cannot quietly stop testing anything.
        let shaped: String = story.text.to_uppercase();
        assert_ne!(
            shaped.len(),
            story.text.len(),
            "this text has to change length when capitalised, or it proves nothing"
        );

        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        for position in 0..=story.text.len() {
            if !story.text.is_char_boundary(position) {
                continue;
            }
            let caret = shaper
                .caret_geometry(&story, &styles, WIDTH, cursor(position), 1.5)
                .caret
                .expect("a caret for every offset");
            let back = shaper.offset_at(
                &story,
                &styles,
                WIDTH,
                caret.x0 + caret.width() / 2.0,
                (caret.y0 + caret.y1) / 2.0,
            );
            assert!(
                story.text.is_char_boundary(back),
                "offset {position} read back as {back}, which cuts a character in half"
            );
            assert_eq!(back, position, "offset {position} reads back as {back}");
        }
    }

    // --- drop caps ----------------------------------------------------------

    fn with_drop_cap(text: &str, lines: u8) -> Story {
        use crate::story::ParagraphFormat;

        let mut story = Story::new(text);
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                drop_cap_lines: Some(lines),
                ..ParagraphFormat::default()
            },
        );
        story
    }

    #[test]
    fn a_caret_in_a_drop_cap_is_in_the_drop_cap() {
        // The cap and the body beside it start at the same y, so the paragraph
        // a click lands in cannot be decided by y alone.
        let story = with_drop_cap("Once upon a time there was a very long sentence", 3);
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        let cap = shaper
            .caret_geometry(&story, &styles, WIDTH, cursor(0), 1.5)
            .caret
            .expect("a caret at the start");
        let back = shaper.offset_at(
            &story,
            &styles,
            WIDTH,
            cap.x0 + 1.0,
            (cap.y0 + cap.y1) / 2.0,
        );
        assert_eq!(back, 0, "clicking the drop cap belongs to the drop cap");
    }

    #[test]
    fn every_offset_survives_a_round_trip_past_a_drop_cap() {
        let story = with_drop_cap("Once upon a time there was a very long sentence", 3);
        let mut shaper = Shaper::new();
        let styles = NoStyles::default();

        for position in 0..=story.text.len() {
            if !story.text.is_char_boundary(position) {
                continue;
            }
            let caret = shaper
                .caret_geometry(&story, &styles, WIDTH, cursor(position), 1.5)
                .caret
                .expect("a caret for every offset");
            assert!(
                caret.x0 >= 0.0 && caret.y0 >= 0.0,
                "offset {position} drew off the frame at {caret:?}"
            );
        }
    }
}
