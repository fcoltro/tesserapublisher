# Milestone 3, Phase 1 — Pages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add, delete, duplicate and reorder pages, and undo any of it.

**Architecture:** The containment chain already exists — `spread_order` →
`Spread.pages` → `Page.layers` → `Layer.frames` — and a new document builds one
of each. Nothing has ever made a *second* one. This phase adds the operations,
the geometry that positions more than one spread, and the menu and navigation
that were recorded partial in milestone 1.5 for want of exactly these commands.

**Tech Stack:** `tessera_document`, `tessera_ui`, existing snapshot undo.

**Spec:** none separate. The decisions are below; the model they act on was
designed in milestone 1.5 phase B.

## Global Constraints

- `unsafe_code = "forbid"` workspace-wide.
- Every document mutation goes through `Command`; a live gesture may bypass it
  only with an adjacent `undo-bracketed:` comment, enforced by
  `tests/command_invariant.rs`.
- **`cargo fmt --all --check`, `cargo test --workspace`, and
  `cargo clippy --workspace --all-targets -- -D warnings` before every commit.**
- British spelling in identifiers and copy.
- A control that sets a value nothing honours is worse than one that is absent.

## Decisions

**D1 — Spreads are positioned, pages are not.** `Page.bounds` is in document
space and everything downstream reads it: rulers, align-to-page, guides, the
PDF's `TrimBox`. So bounds stay the truth, and a `reflow_spreads` recomputes
them after any structural change. Deriving them at resolve time instead would
mean every consumer learning the rule.

**D2 — Spreads stack downward, pages sit side by side.** A spread's pages run
left to right; spreads stack vertically with a gap. InDesign runs them
horizontally, but a wheel scrolls vertically, and the pasteboard is the thing
being navigated. Reversible: it is one function.

**D3 — Adding a page fills the last spread before starting a new one**, when
facing pages is on and that spread is not the first.

*Amended while implementing.* The claim was that this needed no rule about
cover pages, and it did: joining the last spread unconditionally gives 1-2,
3-4, which reads as though the book opened on its own cover. Page 1 is a
right-hand page, so the first spread holds one and the rest pair up — 1, 2-3,
4-5. The test is what said so.

**D4 — Deleting the last page is refused.** A document with no pages has
nothing to show and no way to get one back except undo. InDesign refuses too.

**D5 — Duplicating a page is a deep copy**, including the stories its text
frames reference. Sharing a story would make editing one page edit another,
which is what `Command::DuplicateSelection` already avoids for frames.

**D6 — Which page is current is view state.** It belongs on `OpenDocument`
beside the camera, not in the document: where someone is looking is not part of
what they are making, and it must not make the document dirty or land in undo.

## File Structure

- **Modify** `crates/tessera_document/src/document.rs` — `add_page`,
  `remove_page`, `duplicate_page`, `move_spread`, `reflow_spreads`.
- **Modify** `crates/tessera_ui/src/command.rs` — the commands that call them.
- **Modify** `crates/tessera_ui/src/actions.rs` — a `Layout` group, which is
  what makes the Layout menu appear.
- **Modify** `crates/tessera_ui/src/open_document.rs` — the current page.
- **Modify** `crates/tessera_ui/src/view/panels.rs` — the status bar's page
  count becomes navigation.

---

### Task 1: More than one page can exist

**Files:** `crates/tessera_document/src/document.rs`

**Produces:**
- `Document::reflow_spreads(&mut self)` — positions every spread, stacking them
  down the pasteboard with each spread's pages side by side.
- `Document::add_page(&mut self) -> PageId` — appends, filling the last spread
  when facing pages is on, then reflows.
- `Document::remove_page(&mut self, id) -> bool` — refuses the last page,
  returning whether it removed one. Takes the page's layers and their frames
  with it, and the spread if it empties.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_new_document_has_one_page_and_adding_gives_it_a_second() {
    let mut doc = Document::new();
    assert_eq!(doc.page_ids().count(), 1);
    doc.add_page();
    assert_eq!(doc.page_ids().count(), 2);
}

#[test]
fn a_second_page_does_not_sit_on_top_of_the_first() {
    // Bounds are document space, and everything downstream reads them.
    let mut doc = Document::new();
    let first = doc.page_ids().next().expect("a page");
    let second = doc.add_page();
    let a = doc.pages[first].bounds;
    let b = doc.pages[second].bounds;
    assert!(
        b.y >= a.y + a.height || b.x >= a.x + a.width,
        "the second page overlaps the first: {a:?} then {b:?}"
    );
}

#[test]
fn facing_pages_puts_the_second_page_beside_the_first() {
    let mut doc = Document::new();
    doc.setup.facing_pages = true;
    let first = doc.page_ids().next().expect("a page");
    let second = doc.add_page();
    assert_eq!(
        doc.spread_of(first), doc.spread_of(second),
        "with facing pages the second page joins the first's spread"
    );
}

#[test]
fn the_last_page_cannot_be_removed() {
    let mut doc = Document::new();
    let only = doc.page_ids().next().expect("a page");
    assert!(!doc.remove_page(only), "refused");
    assert_eq!(doc.page_ids().count(), 1);
}

#[test]
fn removing_a_page_takes_its_frames_with_it() {
    let mut doc = Document::new();
    let page = doc.add_page();
    let layer = doc.pages[page].layers[0];
    let frame = doc.add_frame(layer, a_frame());
    assert!(doc.remove_page(page));
    assert!(doc.frame(frame).is_none(), "the frame went with the page");
    assert!(doc.layers.get(layer).is_none(), "and so did its layer");
}
```

- [ ] **Step 2: Run to verify they fail.**
- [ ] **Step 3: Implement**, bumping `revision` on each, since the resolve cache
      keys on it and a new page that does not appear is the margins bug again.
- [ ] **Step 4: Run tests.**
- [ ] **Step 5: Commit** — `Let a document have more than one page`

---

### Task 2: Duplicating and reordering

**Files:** `crates/tessera_document/src/document.rs`

**Produces:**
- `Document::duplicate_page(&mut self, id) -> Option<PageId>` — deep copy, per
  D5, including the stories its text frames reference.
- `Document::move_spread(&mut self, from: usize, to: usize)` — reorders
  `spread_order` and reflows.

- [ ] **Step 1: Write the failing tests** — a duplicated page has its own
      frames; editing the copy's text does not change the original's, which is
      the test that actually pins D5; moving a spread changes the order and the
      geometry follows.
- [ ] **Step 2–5:** verify, implement, run, commit — `Duplicate and reorder pages`

---

### Task 3: The commands, and undo

**Files:** `crates/tessera_ui/src/command.rs`

**Produces:** `AddPage`, `RemovePage { id }`, `DuplicatePage { id }`,
`MoveSpread { from, to }`.

The roadmap singles this out: *the previous implementation never made add or
remove page undoable, because no inverse was ever written.* Snapshot undo means
the inverse is free — but "free" is exactly the kind of claim that is worth a
test, because nothing else would notice if these stopped going through
`Command`.

- [ ] **Step 1: Write the failing tests** — each of the four is undoable, and
      undoing a page removal brings back its frames and their text.
- [ ] **Step 2–5:** verify, implement, run, commit — `Add and remove pages, undoably`

---

### Task 4: The Layout menu and page navigation

**Files:** `crates/tessera_ui/src/actions.rs`,
`crates/tessera_ui/src/open_document.rs`, `crates/tessera_ui/src/view/panels.rs`,
`crates/tessera_ui/src/view/mod.rs`

Milestone 1.5 recorded C10 and C12 as partial for want of these commands. The
menu bar and the palette both generate themselves from the action list, so
adding a `Layout` group is what makes the menu appear.

**Produces:**
- `Group::Layout`, with add, delete and duplicate page.
- `OpenDocument::current_spread` — view state, per D6.
- The status bar's page count becomes a control: previous, "3 of 12", next.

- [ ] **Step 1:** Test that `Group::Layout.menu()` is `"Layout"` and the group
      is not empty — the menu is made of the list, so the list is what to
      assert.
- [ ] **Step 2:** Test that changing the current spread does **not** mark the
      document dirty, per D6.
- [ ] **Step 3–5:** implement, run, commit — `Open the Layout menu, and walk the pages`

---

## Closing

- [ ] Full suite; GPU alone; fmt; clippy.
- [ ] **Perform the sentence, by hand:**

> Add three pages. Put something on the third. Duplicate it and check the copy's
> text edits on its own. Drag a spread earlier and watch the pages renumber.
> Delete a page, undo, and find the frames still there.

- [ ] Update `ROADMAP.md`: tick what is done and tick C10 and C12 in milestone
      1.5, which this phase is what they were waiting for.
