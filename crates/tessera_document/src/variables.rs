//! Text variables: what a document defines for its markers to read as.
//!
//! The built-in markers — the page number, the section marker — need nothing
//! defined; the document knows its own pages. A **text variable** is one the
//! person defines: a running header that reads the nearest heading, or a piece
//! of text used in forty places that should change in all of them at once.
//! Each is referred to from a story by [`tessera_text::variables::Marker::Variable`]
//! carrying its index in [`crate::Document::variables`].
//!
//! ## A running header is read off the page
//!
//! "Running header (paragraph style)" is InDesign's name and its mechanism: the
//! variable names a paragraph style, and on each page reads as the first (or
//! last) paragraph in that style *laid out on that page*. That answer is not
//! known until the page's own text has been laid out — which is why the layout
//! crate resolves a page's own frames before the parent items it inherits,
//! even though the parent items paint underneath. The value is a fact about
//! the layout, and the model only says how to find it.

use serde::{Deserialize, Serialize};

use tessera_text::story::ParagraphStyleId;

/// Which paragraph a running header takes from a page with several.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Which {
    /// The first on the page: the section a verso is in.
    #[default]
    First,
    /// The last on the page: the section a recto reads on to.
    Last,
}

/// What a variable reads as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableKind {
    /// The same text everywhere it is used.
    Custom(String),
    /// The first or last paragraph in `style` on the page.
    RunningHeader {
        style: ParagraphStyleId,
        which: Which,
    },
}

/// A named variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextVariable {
    pub name: String,
    pub kind: VariableKind,
}

impl TextVariable {
    pub fn custom(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: VariableKind::Custom(text.into()),
        }
    }

    pub fn running_header(name: impl Into<String>, style: ParagraphStyleId, which: Which) -> Self {
        Self {
            name: name.into(),
            kind: VariableKind::RunningHeader { style, which },
        }
    }
}

/// The most variables a document can hold: a marker carries its index in one
/// byte of code point, and this is how many that byte can name.
pub const MOST_VARIABLES: usize = 256;
