//! The story model: text plus the formatting applied to it.

use serde::{Deserialize, Serialize};
use tessera_color::Color;

/// Character formatting.
///
/// Milestone 0 applies one style to a whole story. Milestone 2 splits this
/// into runs, which is an additive change: a story gains a `runs` vector and
/// this becomes the default for spans that do not override it.
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
    pub style: TextStyle,
}

impl Story {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: TextStyle::default(),
        }
    }
}
