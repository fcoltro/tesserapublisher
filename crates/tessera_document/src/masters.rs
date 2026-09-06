//! Parent pages: the items that repeat.
//!
//! A master is a **spread that is not in the reading order**. It holds pages
//! like any other spread, those pages hold frames like any other page, and
//! everything that already works on a page — layers, text, transforms — works
//! on a master page without knowing it is one.
//!
//! What makes it a master is that document pages *point at* its pages. The
//! pointing is a reference and never a copy: a master whose items were copied
//! onto each page would not update those pages when it changed, and updating
//! every page at once is the entire reason to have one.

use serde::{Deserialize, Serialize};

use crate::ids::SpreadId;

/// A named parent spread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Master {
    /// What the panel calls it: "A-Master", "B-Chapter opener".
    pub name: String,
    /// The spread holding its pages. **Not in `spread_order`**, so it is never
    /// laid out in the reading order and never numbered as a document page.
    pub spread: SpreadId,
}

impl Master {
    pub fn new(name: impl Into<String>, spread: SpreadId) -> Self {
        Self {
            name: name.into(),
            spread,
        }
    }
}
