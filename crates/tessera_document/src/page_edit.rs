//! Pages edited as a selection: inserted where they are wanted, and copied,
//! deleted, moved and given a parent several at a time.
//!
//! Built on the one-page operations rather than beside them. Each of those
//! repacks the spreads from the reading order after it runs, and that rule —
//! the order decides the spreads, never the other way round — is the one the
//! panel could not recover from when it was broken. Doing several pages as a
//! run of single-page steps keeps it without a second copy of it here.

use crate::{
    document::Document,
    ids::{MasterId, PageId},
};

impl Document {
    /// The parent a page is built on, if any.
    ///
    /// A page stores the parent *page* it inherits — the verso or the recto,
    /// by its side of the fold — and this answers with the parent that page
    /// belongs to, which is what a panel names.
    pub fn master_of_page(&self, page: PageId) -> Option<MasterId> {
        let parent = self.pages.get(page)?.master?;
        self.master_ids()
            .find(|master| self.pages_of_master(*master).contains(&parent))
    }

    /// Whether a numbering section begins on the page.
    pub fn starts_section(&self, page: PageId) -> bool {
        self.sections.iter().any(|s| s.first == page)
    }

    /// A new page directly after `after` — or first, with `None` — built on
    /// the same parent as the page it follows, as InDesign's Insert Pages
    /// builds it.
    ///
    /// "Add page" put every page at the end, so a page wanted in the middle
    /// of a book was added and then dragged the length of the panel.
    pub fn insert_page_after(&mut self, after: Option<PageId>) -> PageId {
        let at = after
            .and_then(|a| self.page_ids().position(|p| p == a))
            .map_or(0, |i| i + 1);
        let parent = after.and_then(|a| self.master_of_page(a));
        let page = self.add_page();
        self.move_page(page, at);
        if parent.is_some() {
            self.apply_master(page, parent);
        }
        page
    }

    /// `count` new pages after `after` — or first, with `None` — in order:
    /// each on `parent` when one is given (`Some(None)` for no parent), or,
    /// with `None`, on the parent of the page it follows, as InDesign's
    /// Insert Pages makes them. The pages made, in reading order.
    pub fn insert_pages(
        &mut self,
        after: Option<PageId>,
        count: usize,
        parent: Option<Option<MasterId>>,
    ) -> Vec<PageId> {
        let mut made = Vec::with_capacity(count);
        let mut previous = after;
        for _ in 0..count {
            let page = self.insert_page_after(previous);
            if let Some(parent) = parent {
                self.apply_master(page, parent);
            }
            made.push(page);
            previous = Some(page);
        }
        made
    }

    /// Put the pages in `order`, which must hold every document page once.
    ///
    /// One page at a time into its place, front to back: each move repacks
    /// the spreads, and after the `i`th move the first `i + 1` pages are
    /// where they end up.
    fn reorder_pages(&mut self, order: &[PageId]) {
        for (index, page) in order.iter().enumerate() {
            if self.page_ids().nth(index) != Some(*page) {
                self.move_page(*page, index);
            }
        }
    }

    /// `pages` in reading order, each once, keeping only document pages.
    fn in_reading_order(&self, pages: &[PageId]) -> Vec<PageId> {
        self.page_ids().filter(|p| pages.contains(p)).collect()
    }

    /// Move several pages at once, as a block in their reading order, to the
    /// place `to` names in the order as it stands — the gap a drop marker
    /// is drawn in, counted before the pages leave it.
    ///
    /// Whether anything moved.
    pub fn move_pages(&mut self, pages: &[PageId], to: usize) -> bool {
        let moving = self.in_reading_order(pages);
        if moving.is_empty() {
            return false;
        }
        let order: Vec<PageId> = self.page_ids().collect();
        // The gap counted in the order without them: every moving page
        // before it takes one place out of the count.
        let before = order.iter().take(to).filter(|p| moving.contains(p)).count();
        let mut rest: Vec<PageId> = order
            .iter()
            .copied()
            .filter(|p| !moving.contains(p))
            .collect();
        let at = to.saturating_sub(before).min(rest.len());
        rest.splice(at..at, moving);
        if rest == order {
            return false;
        }
        self.reorder_pages(&rest);
        true
    }

    /// Copy several pages at once, and put the copies together, in order,
    /// directly after the last of them: pages two and three copied read
    /// 2, 3, 2, 3 — the spread repeated, as InDesign repeats it — where
    /// copying each after its own spread interleaved them.
    ///
    /// The copies, in order.
    pub fn duplicate_pages(&mut self, pages: &[PageId]) -> Vec<PageId> {
        let originals = self.in_reading_order(pages);
        let Some(last) = originals.last().copied() else {
            return Vec::new();
        };
        let copies: Vec<PageId> = originals
            .iter()
            .filter_map(|page| self.duplicate_page(*page))
            .collect();
        let mut order: Vec<PageId> = self.page_ids().filter(|p| !copies.contains(p)).collect();
        let at = order
            .iter()
            .position(|p| *p == last)
            .map_or(order.len(), |i| i + 1);
        order.splice(at..at, copies.iter().copied());
        self.reorder_pages(&order);
        copies
    }

    /// Delete several pages and everything on them.
    ///
    /// **All of them is refused, and changes nothing**, as deleting the last
    /// page one at a time is: a document with no pages has nothing to show.
    /// Deleting all but one would be a guess at which one to keep.
    ///
    /// How many went.
    pub fn remove_pages(&mut self, pages: &[PageId]) -> usize {
        let going = self.in_reading_order(pages);
        if going.is_empty() || going.len() >= self.page_ids().count() {
            return 0;
        }
        going
            .into_iter()
            .filter(|page| self.remove_page(*page))
            .count()
    }

    /// Build several pages on one parent, or on none.
    ///
    /// How many changed: a page already on it, or already on none, did not.
    pub fn apply_master_to(&mut self, pages: &[PageId], master: Option<MasterId>) -> usize {
        pages
            .iter()
            .filter(|page| self.apply_master(**page, master))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A facing document of `n` pages: 1 | 2-3 | 4-5 ...
    fn pages(n: usize) -> (Document, Vec<PageId>) {
        let mut doc = Document::new();
        doc.setup.facing_pages = true;
        while doc.page_ids().count() < n {
            doc.add_page();
        }
        let ids = doc.page_ids().collect();
        (doc, ids)
    }

    fn order(doc: &Document) -> Vec<PageId> {
        doc.page_ids().collect()
    }

    /// Every page sits in one of the two columns of its sheet — the shape a
    /// repack keeps and a hand-made reorder could break.
    fn on_the_sheet(doc: &Document) {
        let width = doc.first_page_bounds().width;
        for page in doc.page_ids() {
            let column = (doc.pages[page].bounds.x / width).round() as i32;
            assert!(
                (0..=1).contains(&column),
                "a page off the sheet at {column}"
            );
        }
    }

    #[test]
    fn a_page_is_inserted_after_the_one_asked_and_takes_its_parent() {
        let (mut doc, ids) = pages(3);
        let master = doc.add_master("A-Master");
        doc.apply_master(ids[1], Some(master));

        let new = doc.insert_page_after(Some(ids[1]));

        assert_eq!(order(&doc), [ids[0], ids[1], new, ids[2]]);
        assert_eq!(doc.master_of_page(new), Some(master));
        on_the_sheet(&doc);

        let first = doc.insert_page_after(None);
        assert_eq!(order(&doc)[0], first, "None puts it first");
        assert_eq!(doc.master_of_page(first), None);
    }

    #[test]
    fn several_pages_are_inserted_in_order_on_the_parent_asked() {
        let (mut doc, ids) = pages(3);
        let a = doc.add_master("A-Master");
        let b = doc.add_master("B-Master");
        doc.apply_master(ids[0], Some(a));

        // As the page before: the first page's parent.
        let made = doc.insert_pages(Some(ids[0]), 2, None);
        assert_eq!(order(&doc), [ids[0], made[0], made[1], ids[1], ids[2]]);
        assert!(made.iter().all(|p| doc.master_of_page(*p) == Some(a)));

        // On another parent, at the end.
        let end = doc.insert_pages(Some(ids[2]), 3, Some(Some(b)));
        assert_eq!(&order(&doc)[5..], end.as_slice());
        assert!(end.iter().all(|p| doc.master_of_page(*p) == Some(b)));

        // On none, at the start.
        let start = doc.insert_pages(None, 1, Some(None));
        assert_eq!(order(&doc)[0], start[0]);
        assert_eq!(doc.master_of_page(start[0]), None);
        assert!(doc.insert_pages(None, 0, None).is_empty());
        on_the_sheet(&doc);
    }

    #[test]
    fn several_pages_move_as_a_block_to_the_gap_they_were_dropped_in() {
        let (mut doc, ids) = pages(6);
        // Pages two and three, dropped in the gap after page five.
        assert!(doc.move_pages(&[ids[2], ids[1]], 5));
        assert_eq!(
            order(&doc),
            [ids[0], ids[3], ids[4], ids[1], ids[2], ids[5]]
        );
        on_the_sheet(&doc);
        // And back to the front.
        assert!(doc.move_pages(&[ids[1], ids[2]], 0));
        assert_eq!(order(&doc)[..2], [ids[1], ids[2]]);
        assert!(!doc.move_pages(&[ids[1], ids[2]], 0), "already there");
    }

    #[test]
    fn copies_of_several_pages_follow_the_last_of_them_in_order() {
        let (mut doc, ids) = pages(5);
        let copies = doc.duplicate_pages(&[ids[2], ids[1]]);
        assert_eq!(copies.len(), 2);
        assert_eq!(
            order(&doc),
            [ids[0], ids[1], ids[2], copies[0], copies[1], ids[3], ids[4]],
            "2, 3, then their copies, then the rest"
        );
        on_the_sheet(&doc);
    }

    #[test]
    fn deleting_every_page_is_refused_and_changes_nothing() {
        let (mut doc, ids) = pages(3);
        let before = doc.revision();
        assert_eq!(doc.remove_pages(&ids), 0);
        assert_eq!(doc.revision(), before);
        assert_eq!(order(&doc), ids);

        assert_eq!(doc.remove_pages(&[ids[0], ids[2]]), 2);
        assert_eq!(order(&doc), [ids[1]]);
    }

    #[test]
    fn a_parent_goes_on_every_page_asked_and_is_named_back() {
        let (mut doc, ids) = pages(4);
        let master = doc.add_master("B-Chapter");
        assert_eq!(doc.apply_master_to(&ids[1..3], Some(master)), 2);
        let parents: Vec<_> = ids.iter().map(|p| doc.master_of_page(*p)).collect();
        assert_eq!(parents, [None, Some(master), Some(master), None]);
        assert_eq!(
            doc.apply_master_to(&ids, None),
            2,
            "only the two that had one"
        );
        assert!(ids.iter().all(|p| doc.master_of_page(*p).is_none()));
    }
}
