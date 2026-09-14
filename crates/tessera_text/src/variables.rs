//! Text that stands for something the page knows.
//!
//! A page number is typed once, on a parent page, and reads differently on
//! every page that inherits it. The story cannot hold "12" — the same story is
//! shown on page 12 and on page 13 — so it holds a **marker**, one character,
//! and the marker is swapped for the page's answer when the paragraph is
//! shaped. The swap happens in the same place small caps and list markers are
//! synthesised, so the offset map that lets a caret find its way back through
//! a synthesised capital serves a page number for free: a click on "12" lands
//! the caret at the marker, and Backspace deletes the marker rather than a
//! digit of it.
//!
//! ## The marker is the reference
//!
//! Markers are Private Use characters. Nothing else in a story is one: a font
//! that happens to carry a glyph there never sees it, because the marker is
//! replaced before parley does. Being a character rather than an object beside
//! the text is what makes a marker survive copy, paste, Find and Change, and
//! every edit the story supports, without any of them knowing it exists — the
//! arrangement the anchored-object marker earned first.
//!
//! The built-in markers are fixed characters. A **text variable** — a running
//! header, a piece of custom text — is one of a document's own, so its marker
//! carries its index: `Variable(3)` is the fourth variable the document
//! defines, and deleting a variable leaves its marker reading as nothing rather
//! than renumbering the rest, for the same reason a deleted style does not
//! shuffle the others' identities.

/// What a marker character stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Marker {
    /// The number of the page this text is laid out on.
    PageNumber,
    /// The number of the page holding the next frame of this thread.
    NextPageNumber,
    /// The number of the page holding the previous frame of this thread.
    PreviousPageNumber,
    /// The section marker text of the section the page is in.
    SectionMarker,
    /// The document's `n`th text variable.
    Variable(u8),
}

/// The first character in the block the built-in markers occupy.
const BUILT_IN: u32 = 0xE000;
/// The first character in the block variable markers occupy: `U+E100` is
/// variable 0, `U+E1FF` variable 255.
const VARIABLE: u32 = 0xE100;

impl Marker {
    /// The character that stands for this marker in a story.
    pub fn character(self) -> char {
        let code = match self {
            Marker::PageNumber => BUILT_IN,
            Marker::NextPageNumber => BUILT_IN + 1,
            Marker::PreviousPageNumber => BUILT_IN + 2,
            Marker::SectionMarker => BUILT_IN + 3,
            Marker::Variable(index) => VARIABLE + u32::from(index),
        };
        char::from_u32(code).expect("a Private Use code point is a character")
    }

    /// What `character` stands for, if it is a marker.
    pub fn of(character: char) -> Option<Marker> {
        let code = u32::from(character);
        match code {
            c if c == BUILT_IN => Some(Marker::PageNumber),
            c if c == BUILT_IN + 1 => Some(Marker::NextPageNumber),
            c if c == BUILT_IN + 2 => Some(Marker::PreviousPageNumber),
            c if c == BUILT_IN + 3 => Some(Marker::SectionMarker),
            c if (VARIABLE..VARIABLE + 256).contains(&c) => {
                Some(Marker::Variable((c - VARIABLE) as u8))
            }
            _ => None,
        }
    }

    /// What the marker reads as when nothing is there to answer it.
    ///
    /// A page number with no page — a story shaped by a test, or by something
    /// that has no page context — reads as a number sign, which is what the
    /// marker is drawn as in every layout tool and is honest about being a
    /// placeholder. The others read as nothing, because nothing is what they
    /// would say.
    pub fn placeholder(self) -> &'static str {
        match self {
            Marker::PageNumber | Marker::NextPageNumber | Marker::PreviousPageNumber => "#",
            Marker::SectionMarker | Marker::Variable(_) => "",
        }
    }
}

/// What the markers read as, on one page.
///
/// Built by whoever knows the page — the layout crate, which knows which page
/// a frame stands on and which section that page is in — and handed to the
/// shaper through [`crate::story::Styles::variables`]. The shaper never learns
/// what a page is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Variables {
    pub page_number: String,
    pub next_page_number: String,
    pub previous_page_number: String,
    pub section_marker: String,
    /// By index, in the order the document defines them. A marker past the
    /// end reads as nothing.
    pub variables: Vec<String>,
}

impl Variables {
    /// What `marker` reads as here.
    pub fn text_of(&self, marker: Marker) -> &str {
        match marker {
            Marker::PageNumber => &self.page_number,
            Marker::NextPageNumber => &self.next_page_number,
            Marker::PreviousPageNumber => &self.previous_page_number,
            Marker::SectionMarker => &self.section_marker,
            Marker::Variable(index) => self
                .variables
                .get(usize::from(index))
                .map(String::as_str)
                .unwrap_or(""),
        }
    }
}

/// `text` with every marker replaced by what it reads as, or by its
/// placeholder when there is nothing to read.
///
/// For the places that want a story's words rather than its layout — a running
/// header built from a heading, the story editor, a preflight message — so
/// none of them prints a Private Use character.
pub fn expand(text: &str, variables: Option<&Variables>) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match Marker::of(character) {
            Some(marker) => out.push_str(
                variables
                    .map(|v| v.text_of(marker))
                    .unwrap_or_else(|| marker.placeholder()),
            ),
            None => out.push(character),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_marker_survives_the_trip_through_its_character() {
        let all = [
            Marker::PageNumber,
            Marker::NextPageNumber,
            Marker::PreviousPageNumber,
            Marker::SectionMarker,
            Marker::Variable(0),
            Marker::Variable(7),
            Marker::Variable(255),
        ];
        for marker in all {
            assert_eq!(Marker::of(marker.character()), Some(marker));
        }
    }

    #[test]
    fn ordinary_text_is_not_a_marker() {
        for c in ['a', '#', '\u{FFFC}', '\u{00AD}', '\u{E200}', '\u{D7FF}'] {
            assert_eq!(Marker::of(c), None, "{c:?}");
        }
    }

    #[test]
    fn expansion_reads_the_page_and_falls_back_to_the_placeholder() {
        let text = format!(
            "Page {} of {}{}",
            Marker::PageNumber.character(),
            Marker::Variable(0).character(),
            Marker::SectionMarker.character()
        );
        assert_eq!(expand(&text, None), "Page # of ");
        let variables = Variables {
            page_number: "iv".into(),
            variables: vec!["Chapter One".into()],
            section_marker: " — Front matter".into(),
            ..Default::default()
        };
        assert_eq!(
            expand(&text, Some(&variables)),
            "Page iv of Chapter One — Front matter"
        );
    }

    #[test]
    fn a_variable_past_the_end_reads_as_nothing() {
        let variables = Variables::default();
        assert_eq!(variables.text_of(Marker::Variable(3)), "");
    }
}
