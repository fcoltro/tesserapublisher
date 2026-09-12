//! Frames anchored in a story, so they travel with the copy.
//!
//! ## The marker is the anchor, and the index is only a name for it
//!
//! An anchored frame's place in the text is a `U+FFFC OBJECT REPLACEMENT
//! CHARACTER` in the story. **Not a byte offset stored on the frame**, which is
//! the obvious design and the wrong one: every insertion before it would move
//! the real position and leave the stored number behind, so the frame would
//! drift away from its sentence on the first edit. A character in the text is
//! carried along by the ordinary editing operations for nothing, because they
//! already maintain the text.
//!
//! What [`Anchored`] records is therefore not *where* but *which*: this frame
//! belongs to the **nth** marker in that story. Indices only change when a
//! marker is added or removed, which is an operation on anchors anyway.
//!
//! [`Anchors::are_sound`] is the invariant everything here preserves — one
//! frame per marker, indices `0..n` with none missing and none repeated — in
//! the same spirit as `Story::runs_are_sound` and `Table::spans_are_sound`.

use serde::{Deserialize, Serialize};

use crate::ids::{FrameId, StoryId};

/// The character that stands in for an anchored object in the text.
///
/// Unicode's own: it is what every text system uses for "something that is not
/// text goes here", so a story handed to anything else already means the right
/// thing.
pub const MARKER: char = '\u{FFFC}';

/// How an anchored frame sits relative to the line it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AnchorKind {
    /// In the line, like a very large character.
    #[default]
    Inline,
    /// On its own line, the copy broken above and below it.
    ///
    /// Recorded now and laid out as `Inline` until the layout pass learns the
    /// difference — an enum that grows later is a format change, and one that
    /// carries an unimplemented case is not.
    Above,
}

/// A frame's place in a story.
// No `Eq`: `baseline_shift` is a float. `PartialEq` is what the round-trip
// tests need and all a measurement in points can honestly offer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Anchored {
    pub story: StoryId,
    /// Which marker in that story, counting from zero in reading order.
    pub index: usize,
    #[serde(default)]
    pub kind: AnchorKind,
    /// Points to raise the object above its baseline. Negative lowers it.
    #[serde(default)]
    pub baseline_shift: f64,
}

impl Anchored {
    pub fn new(story: StoryId, index: usize) -> Self {
        Self {
            story,
            index,
            kind: AnchorKind::Inline,
            baseline_shift: 0.0,
        }
    }
}

/// Every marker offset in a story's text, in reading order.
pub fn marker_offsets(text: &str) -> Vec<usize> {
    text.char_indices()
        .filter(|(_, c)| *c == MARKER)
        .map(|(at, _)| at)
        .collect()
}

/// How many anchored objects a story holds.
pub fn marker_count(text: &str) -> usize {
    text.chars().filter(|c| *c == MARKER).count()
}

/// The anchors of one story, in index order.
///
/// A view over frames rather than a stored list: the frames are where the
/// anchors live, and a second list beside them is the kind of arrangement that
/// drifts. Built when needed, which is cheap at these sizes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Anchors {
    /// `(index, frame)`, sorted by index.
    pub frames: Vec<(usize, FrameId)>,
}

impl Anchors {
    /// Whether the anchors and the markers agree.
    ///
    /// One frame per marker, indices `0..markers` with none missing, none
    /// repeated and none past the end. A story that fails this has an object
    /// nothing can place, or a marker that reserves room for nothing — and
    /// either shows up as a hole in the page, far from whatever caused it.
    pub fn are_sound(&self, markers: usize) -> bool {
        if self.frames.len() != markers {
            return false;
        }
        self.frames
            .iter()
            .enumerate()
            .all(|(i, (index, _))| *index == i)
    }

    /// The frame belonging to the nth marker.
    pub fn frame_at(&self, index: usize) -> Option<FrameId> {
        self.frames
            .iter()
            .find(|(i, _)| *i == index)
            .map(|(_, frame)| *frame)
    }
}

/// Renumber after a marker is taken out.
///
/// Everything after the removed one moves down by one. Returned as the new
/// numbering rather than applied, because the frames belong to a document and
/// this module never reaches one.
pub fn renumbered_after_removal(
    frames: &[(usize, FrameId)],
    removed: usize,
) -> Vec<(usize, FrameId)> {
    frames
        .iter()
        .filter(|(index, _)| *index != removed)
        .map(|(index, frame)| (if *index > removed { index - 1 } else { *index }, *frame))
        .collect()
}

/// Renumber after a marker is put in at `inserted`.
pub fn renumbered_after_insertion(
    frames: &[(usize, FrameId)],
    inserted: usize,
) -> Vec<(usize, FrameId)> {
    frames
        .iter()
        .map(|(index, frame)| {
            (
                if *index >= inserted {
                    index + 1
                } else {
                    *index
                },
                *frame,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchors(indices: &[usize]) -> Anchors {
        Anchors {
            frames: indices.iter().map(|i| (*i, FrameId::default())).collect(),
        }
    }

    #[test]
    fn markers_are_found_at_their_byte_offsets() {
        let text = format!("a{MARKER}b{MARKER}");
        let found = marker_offsets(&text);
        assert_eq!(found.len(), 2);
        assert_eq!(marker_count(&text), 2);
        for at in found {
            assert_eq!(text[at..].chars().next(), Some(MARKER));
        }
    }

    #[test]
    fn a_marker_is_three_bytes_so_offsets_are_not_character_counts() {
        // The trap this pins: `U+FFFC` is three bytes in UTF-8, so counting
        // characters and indexing bytes give different answers the moment
        // there is more than one marker.
        let text = format!("{MARKER}{MARKER}");
        assert_eq!(marker_offsets(&text), vec![0, 3]);
    }

    #[test]
    fn one_frame_per_marker_numbered_from_zero_is_sound() {
        assert!(anchors(&[0, 1, 2]).are_sound(3));
    }

    #[test]
    fn a_marker_with_no_frame_is_not_sound() {
        // Room reserved in the line for something that will never be drawn.
        assert!(!anchors(&[0, 1]).are_sound(3));
    }

    #[test]
    fn a_frame_with_no_marker_is_not_sound() {
        // An object nothing can place: the text has nowhere to put it.
        assert!(!anchors(&[0, 1, 2]).are_sound(2));
    }

    #[test]
    fn a_gap_in_the_numbering_is_not_sound() {
        // Two frames and two markers, but one of them claims to be the third.
        assert!(!anchors(&[0, 2]).are_sound(2));
    }

    #[test]
    fn a_repeated_index_is_not_sound() {
        assert!(!anchors(&[0, 0]).are_sound(2));
    }

    #[test]
    fn removing_a_marker_moves_everything_after_it_down() {
        let a = FrameId::default();
        let frames = vec![(0, a), (1, a), (2, a)];
        let after = renumbered_after_removal(&frames, 1);

        assert_eq!(after.len(), 2);
        assert_eq!(after[0].0, 0, "before the removal, unchanged");
        assert_eq!(after[1].0, 1, "after it, moved down");
        assert!(Anchors { frames: after }.are_sound(2));
    }

    #[test]
    fn inserting_a_marker_moves_everything_at_or_after_it_up() {
        let a = FrameId::default();
        let frames = vec![(0, a), (1, a)];
        let after = renumbered_after_insertion(&frames, 0);

        assert_eq!(after[0].0, 1, "the one that was first is now second");
        assert_eq!(after[1].0, 2);
        // Not sound yet on purpose: the caller still has to add the frame for
        // the new marker, and this is what says so.
        assert!(!Anchors { frames: after }.are_sound(3));
    }
}
