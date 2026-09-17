//! Type on a path: a story set along a path frame's curve.
//!
//! InDesign's model, and the reason it is not a kind of text frame: the path
//! stays a path — it has its stroke, its anchors, its fill, and the pen still
//! edits it — and the text is something it *carries*. A path with text on it
//! is still moved, rotated and drawn as a path; the text follows because it
//! is laid out along whatever the path is at the time.
//!
//! So the text is a relationship between a frame and a story, and it lives on
//! the document beside the master overrides rather than as a field of either
//! end. A path that is removed takes its entry with it, as a frame's override
//! goes.

use serde::{Deserialize, Serialize};

use crate::ids::StoryId;

/// Where the glyphs sit against the path, across it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PathTextAlign {
    /// The baseline rides the path: the letters stand on the line.
    #[default]
    Baseline,
    /// The x-height's middle rides the path: the line runs through the
    /// small letters.
    Centre,
    /// The ascender rides the path: the letters hang below it.
    Ascender,
    /// The descender rides the path: the letters stand above it, whole.
    Descender,
}

/// A story carried along a path frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PathText {
    pub story: StoryId,
    /// Where along the path the text starts, as a fraction of its length.
    #[serde(default)]
    pub start: f64,
    /// Where it must end, as a fraction of its length. Text past this is
    /// overset, as text past a frame's bottom is.
    #[serde(default = "one")]
    pub end: f64,
    #[serde(default)]
    pub align: PathTextAlign,
    /// Whether the text runs the other way along the path, letters on the
    /// other side of it — InDesign's flip. What turns text on the inside of
    /// a circle into text on its outside.
    #[serde(default)]
    pub flip: bool,
}

fn one() -> f64 {
    1.0
}

impl PathText {
    /// Text along the whole of a path, standing on it.
    pub fn new(story: StoryId) -> Self {
        Self {
            story,
            start: 0.0,
            end: 1.0,
            align: PathTextAlign::Baseline,
            flip: false,
        }
    }

    /// `start` and `end` kept within the path and in order.
    pub fn normalised(mut self) -> Self {
        self.start = self.start.clamp(0.0, 1.0);
        self.end = self.end.clamp(0.0, 1.0);
        if self.end < self.start {
            std::mem::swap(&mut self.start, &mut self.end);
        }
        self
    }
}
