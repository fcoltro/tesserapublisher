//! Page numbering, in sections.
//!
//! A book numbers its front matter in roman and its body from one in arabic,
//! and a journal picks up where the last issue stopped. So a page's number is
//! not its position: it is decided by the **section** the page is in, and a
//! section is a page the numbering restarts at, with a style, a start and a
//! prefix.
//!
//! ## The first section is implied
//!
//! A document with no sections numbers 1, 2, 3 from its first page. That is
//! not a section stored anywhere — it is what "no sections" means — so a
//! document written before sections existed reads exactly as it did, and a
//! document whose sections are all deleted goes back to it. The first stored
//! section may start on the first page, in which case it *is* the numbering
//! of the whole document until the next one.
//!
//! ## A section is a page
//!
//! A section starts at a `PageId` rather than at an index, so moving pages
//! moves the section with the page it starts on, which is what a person
//! reordering chapters expects. A section whose page has gone is ignored
//! rather than repaired: removing a page removes the section that began there,
//! and the pages after it fall into the section before, which is where they
//! would have been if the page had never existed.

use serde::{Deserialize, Serialize};
use tessera_text::story::Numbering;

use crate::ids::PageId;

/// Where numbering restarts, and how it runs from there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    /// The page the section begins on.
    pub first: PageId,
    /// The number the first page takes. `None` continues from the page
    /// before, which is what a section that only changes the style wants.
    #[serde(default)]
    pub start: Option<u32>,
    #[serde(default)]
    pub style: Numbering,
    /// Written in front of every number in the section: "A-" makes "A-1".
    /// Carried by the auto page number and shown in the pages panel.
    #[serde(default)]
    pub prefix: String,
    /// What the section marker character reads as on every page of the
    /// section: a chapter title, a part name.
    #[serde(default)]
    pub marker: String,
}

impl Section {
    /// A section restarting at one, in arabic, with nothing else.
    pub fn starting_at(first: PageId) -> Self {
        Self {
            first,
            start: Some(1),
            style: Numbering::Arabic,
            prefix: String::new(),
            marker: String::new(),
        }
    }
}

/// One page's number, as its section writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageNumber {
    /// The count within the section's numbering: 4 for "iv".
    pub ordinal: u32,
    /// The number as written, prefix included: "A-4", "iv", "12".
    pub label: String,
    /// The section's marker text.
    pub marker: String,
    /// Which stored section the page is in; `None` for the implied first.
    pub section: Option<usize>,
}

/// Number every page in `pages`, in that order.
///
/// Returns one entry per page. Sections are looked up by the page they start
/// on, so the order handed in is the only ordering that matters — which is
/// the reading order, because that is the order pages are numbered in.
pub fn number_pages(pages: &[PageId], sections: &[Section]) -> Vec<PageNumber> {
    let mut out = Vec::with_capacity(pages.len());
    let mut section: Option<usize> = None;
    let mut style = Numbering::Arabic;
    let mut prefix = String::new();
    let mut marker = String::new();
    let mut next: u32 = 1;

    for page in pages {
        if let Some((index, found)) = sections.iter().enumerate().find(|(_, s)| s.first == *page) {
            section = Some(index);
            style = found.style;
            prefix = found.prefix.clone();
            marker = found.marker.clone();
            if let Some(start) = found.start {
                next = start;
            }
        }
        let ordinal = next;
        out.push(PageNumber {
            ordinal,
            label: format!("{prefix}{}", style.label(ordinal as usize)),
            marker: marker.clone(),
            section,
        });
        next = next.saturating_add(1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn pages(n: usize) -> Vec<PageId> {
        let mut map: SlotMap<PageId, ()> = SlotMap::with_key();
        (0..n).map(|_| map.insert(())).collect()
    }

    fn labels(pages: &[PageId], sections: &[Section]) -> Vec<String> {
        number_pages(pages, sections)
            .into_iter()
            .map(|n| n.label)
            .collect()
    }

    #[test]
    fn no_sections_numbers_from_one() {
        let p = pages(3);
        assert_eq!(labels(&p, &[]), ["1", "2", "3"]);
    }

    #[test]
    fn front_matter_in_roman_then_the_body_from_one() {
        let p = pages(5);
        let sections = [
            Section {
                first: p[0],
                start: Some(1),
                style: Numbering::LowerRoman,
                prefix: String::new(),
                marker: "Front matter".into(),
            },
            Section::starting_at(p[3]),
        ];
        assert_eq!(labels(&p, &sections), ["i", "ii", "iii", "1", "2"]);
        let numbered = number_pages(&p, &sections);
        assert_eq!(numbered[1].marker, "Front matter");
        assert_eq!(numbered[3].marker, "");
        assert_eq!(numbered[0].section, Some(0));
        assert_eq!(numbered[4].section, Some(1));
    }

    #[test]
    fn a_section_that_only_changes_the_prefix_continues_the_count() {
        let p = pages(4);
        let sections = [Section {
            first: p[2],
            start: None,
            style: Numbering::Arabic,
            prefix: "B-".into(),
            marker: String::new(),
        }];
        assert_eq!(labels(&p, &sections), ["1", "2", "B-3", "B-4"]);
    }

    #[test]
    fn a_section_can_start_anywhere() {
        let p = pages(2);
        let sections = [Section {
            first: p[0],
            start: Some(101),
            style: Numbering::Arabic,
            prefix: String::new(),
            marker: String::new(),
        }];
        assert_eq!(labels(&p, &sections), ["101", "102"]);
    }

    #[test]
    fn a_section_whose_page_is_gone_is_ignored() {
        let p = pages(3);
        let gone = pages(1)[0];
        let sections = [Section::starting_at(gone)];
        assert_eq!(labels(&p[..2], &sections), ["1", "2"]);
    }
}
