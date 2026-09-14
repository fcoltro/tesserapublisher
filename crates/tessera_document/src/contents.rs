//! What the table of contents and the index are built from.
//!
//! Neither is text a person types: both are **generated** from the document
//! — a heading and the page it starts on, a topic and the pages it is on —
//! and regenerated on request, because the pages move. What is stored is the
//! recipe and where the result was put, so "update" knows what to rebuild and
//! which story to write it into.

use serde::{Deserialize, Serialize};
use tessera_text::story::ParagraphStyleId;

use crate::ids::StoryId;

/// One level of the contents: paragraphs in `style` are listed, and each
/// entry is set in `entry_style` — or the document default, when none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Level {
    pub style: ParagraphStyleId,
    #[serde(default)]
    pub entry_style: Option<ParagraphStyleId>,
}

/// The recipe for the table of contents, and where it went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contents {
    pub title: String,
    #[serde(default)]
    pub title_style: Option<ParagraphStyleId>,
    #[serde(default)]
    pub levels: Vec<Level>,
    /// The story the contents were last written into. `None` until placed;
    /// a story that has since gone means "place it again".
    #[serde(default)]
    pub story: Option<StoryId>,
}

impl Default for Contents {
    fn default() -> Self {
        Self {
            title: "Contents".into(),
            title_style: None,
            levels: Vec::new(),
            story: None,
        }
    }
}

/// The recipe for the index, and where it went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub title: String,
    #[serde(default)]
    pub story: Option<StoryId>,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            title: "Index".into(),
            story: None,
        }
    }
}
