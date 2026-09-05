# Milestone 1.5 Phase B — The Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the page a real page — margins, bleed, slug, facing-page spreads and guides — in **one** format version bump, and draw it.

**Architecture:** Everything the document gains lands together, because a format version costs a migration test and six scattered bumps cost six. Geometry goes on the document (`DocumentSetup`) rather than duplicated per page; a page's own size stays in `Page.bounds`, which already holds it, so there is never a second source of truth for the same number. `ResolvedDocument` gains the resolved page rectangles, so the screen and the PDF read one answer instead of computing two.

**Tech Stack:** Rust 2024, serde + serde_json, zip, kurbo, egui 0.35, pdf-writer. No new dependencies.

**Spec:** [`docs/superpowers/specs/2026-09-03-instrument-milestone-design.md`](../specs/2026-09-03-instrument-milestone-design.md) — §7 for why this phase is batched.

**Roadmap:** [`ROADMAP.md`](../../../ROADMAP.md), milestone 1.5 phase B.

## Global Constraints

- `unsafe_code = "forbid"` at the workspace level.
- Document units are **points, 1/72 inch**. Margins, bleed and slug are stored in points like everything else; `Unit` converts at the interface edge only.
- **`FORMAT_VERSION` goes from 4 to 5, once.** Not once per task. Task 6 does it, and no earlier task may touch it.
- Every new field carries `#[serde(default)]`, so a version-4 document loads without a rewriting migration. The version still moves, so an older build refuses a newer document rather than silently dropping what it cannot represent — the reason 3→4 bumped too.
- Tests land in the same commit as the code they test.
- No silent fallbacks. Every error path states its cause.
- `cargo clippy --workspace --all-targets -- -D warnings` clean before each commit.
- Single-crate test commands (`-p`), because GPU tests must not join the run.

## Two corrections to the roadmap, made before starting

**The version is 5, not 3.** `FORMAT_VERSION` in `crates/tessera_document/src/format/mod.rs` is already **4**: version 2 added frame rotation, 3 replaced it with a full affine transform, 4 added stroke alignment, caps, joins and dashes. The roadmap's "format version 3" line was written from a stale reading and is corrected by this plan.

**`ColorRef` is dropped from this phase.** It was the one item I flagged as speculative, on the argument that adding the indirection at milestone 5 would mean migrating every fill and stroke in every saved document. Reading `format/mod.rs` undermines that argument: `rotation_to_transform` shows the codebase already does exactly this kind of mechanical JSON rewrite in about twenty lines, and wrapping every colour in `Direct` is the same shape of walk. The cost is therefore roughly equal now and later, the benefit before milestone 5 is nil, and reserving a shape before swatch semantics are designed risks reserving the wrong one. YAGNI wins.

---

### Task 1: Page geometry on the document

**Files:**
- Modify: `crates/tessera_document/src/nodes.rs`, `crates/tessera_document/src/document.rs`
- Test: inline `#[cfg(test)]` in both

**Interfaces:**
- Consumes: `DocRect`, `serde`.
- Produces:
  - `Margins { top, bottom, inside, outside }` — `f64`; `Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize`
  - `Insets { top, bottom, left, right }` — same derives
  - `PageSide::{ Left, Right, Single }` — `Debug, Clone, Copy, PartialEq, Eq`
  - `DocumentSetup { margins: Margins, bleed: Insets, slug: Insets, facing_pages: bool }` — same derives plus `Default`
  - `Document::setup: DocumentSetup` — public field, `#[serde(default)]`

Margins are **inside/outside**, not left/right, because that is what they mean in a bound document: the inside margin is the one against the spine, and on a left-hand page it is on the right. Storing left/right would make every facing-page layout wrong on alternate pages, and is the kind of error that only shows up at the printer.

- [ ] **Step 1: Write the failing tests**

Append to the `#[cfg(test)] mod tests` in `crates/tessera_document/src/nodes.rs`:

```rust
    #[test]
    fn a_fresh_setup_has_no_margins_bleed_or_slug() {
        // A document that never had them has none. Inventing 10mm would be
        // fabricating data the user never entered.
        let setup = DocumentSetup::default();
        assert_eq!(setup.margins, Margins::default());
        assert_eq!(setup.bleed, Insets::default());
        assert_eq!(setup.slug, Insets::default());
        assert!(!setup.facing_pages);
    }

    #[test]
    fn margins_are_uniform_when_every_edge_matches() {
        let m = Margins::uniform(36.0);
        assert_eq!(m.top, 36.0);
        assert_eq!(m.bottom, 36.0);
        assert_eq!(m.inside, 36.0);
        assert_eq!(m.outside, 36.0);
    }

    #[test]
    fn insets_are_uniform_when_every_edge_matches() {
        let b = Insets::uniform(8.5);
        assert_eq!((b.top, b.bottom, b.left, b.right), (8.5, 8.5, 8.5, 8.5));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tessera_document nodes::`
Expected: FAIL to compile, `cannot find type DocumentSetup in this scope`.

- [ ] **Step 3: Write the implementation**

Add to `crates/tessera_document/src/nodes.rs`:

```rust
/// Distances **inward** from a page's edge to its type area.
///
/// Inside and outside rather than left and right, because that is what a
/// margin means in a bound document: the inside margin is the one against the
/// spine, and on a left-hand page it falls on the right. Storing left and
/// right would put every facing-page layout's margins on the wrong side of
/// alternate pages — an error that first shows up at the printer.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Margins {
    pub top: f64,
    pub bottom: f64,
    /// Toward the spine on a facing-page spread; the left edge otherwise.
    pub inside: f64,
    /// Away from the spine; the right edge otherwise.
    pub outside: f64,
}

impl Margins {
    pub fn uniform(all: f64) -> Self {
        Self {
            top: all,
            bottom: all,
            inside: all,
            outside: all,
        }
    }
}

/// Distances **outward** from a page's edge.
///
/// Bleed and slug both grow away from the page, so they are left and right:
/// there is no spine to be inside of.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Insets {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

impl Insets {
    pub fn uniform(all: f64) -> Self {
        Self {
            top: all,
            bottom: all,
            left: all,
            right: all,
        }
    }
}

/// Which side of a spread a page sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSide {
    /// A verso: its spine is on the right.
    Left,
    /// A recto: its spine is on the left.
    Right,
    /// Not part of a facing-page spread, so it has no spine.
    Single,
}

/// The document's page setup.
///
/// Page **size** is deliberately absent: it already lives in `Page.bounds`,
/// and holding it in two places would mean deciding, forever, which one is
/// right when they disagree.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct DocumentSetup {
    pub margins: Margins,
    pub bleed: Insets,
    pub slug: Insets,
    pub facing_pages: bool,
}
```

- [ ] **Step 4: Put it on the document**

In `crates/tessera_document/src/document.rs`, add to `Document`:

```rust
    /// Margins, bleed, slug and whether pages face each other.
    ///
    /// `serde(default)` so that a document written before page setup existed
    /// loads with none of it — which is the truth about that document, rather
    /// than a fabricated default it never chose.
    #[serde(default)]
    pub setup: crate::nodes::DocumentSetup,
```

and initialise it in `Document::new()` with `setup: Default::default(),`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p tessera_document`
Expected: PASS, including the existing round-trip property tests.

- [ ] **Step 6: Commit**

```bash
git add crates/tessera_document/src/nodes.rs crates/tessera_document/src/document.rs
git commit -m "Give the document margins, bleed and a slug"
```

---

### Task 2: Facing pages, and which side a page is on

**Files:**
- Modify: `crates/tessera_document/src/document.rs`
- Test: inline `#[cfg(test)]` in the same file

**Interfaces:**
- Consumes: `PageSide`, `DocumentSetup`, `Spread`, `Page` from task 1.
- Produces:
  - `Document::page_side(&self, page: PageId) -> PageSide`
  - `Document::spread_of(&self, page: PageId) -> Option<SpreadId>`

- [ ] **Step 1: Write the failing tests**

Append to `crates/tessera_document/src/document.rs`'s test module:

```rust
    #[test]
    fn a_page_has_no_side_when_pages_do_not_face() {
        let doc = Document::new();
        let page = doc.page_ids().next().expect("a new document has a page");
        assert_eq!(doc.page_side(page), PageSide::Single);
    }

    #[test]
    fn the_first_page_of_a_facing_spread_is_a_verso_and_the_second_a_recto() {
        let mut doc = Document::new();
        doc.setup.facing_pages = true;

        let spread = doc.spread_ids().next().expect("a new document has a spread");
        let first = doc.pages_of(spread)[0];

        // A one-page spread still has no facing partner.
        assert_eq!(doc.page_side(first), PageSide::Single);

        let second = doc.add_page_to(spread);
        assert_eq!(doc.page_side(first), PageSide::Left, "verso");
        assert_eq!(doc.page_side(second), PageSide::Right, "recto");
    }

    #[test]
    fn a_page_knows_which_spread_holds_it() {
        let doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let spread = doc.spread_of(page).expect("it is in a spread");
        assert!(doc.pages_of(spread).contains(&page));
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tessera_document document::`
Expected: FAIL, `no method named page_side`.

- [ ] **Step 3: Implement**

Add to `impl Document` in `crates/tessera_document/src/document.rs`:

```rust
    /// The pages of a spread, in reading order.
    pub fn pages_of(&self, spread: SpreadId) -> Vec<PageId> {
        self.spreads
            .get(spread)
            .map(|s| s.pages.clone())
            .unwrap_or_default()
    }

    /// Which spread holds this page.
    pub fn spread_of(&self, page: PageId) -> Option<SpreadId> {
        self.spread_order
            .iter()
            .copied()
            .find(|s| self.pages_of(*s).contains(&page))
    }

    /// Which side of its spread this page sits on.
    ///
    /// `Single` whenever there is no spine to be inside of: pages that do not
    /// face, and a spread holding only one page.
    pub fn page_side(&self, page: PageId) -> PageSide {
        if !self.setup.facing_pages {
            return PageSide::Single;
        }
        let Some(spread) = self.spread_of(page) else {
            return PageSide::Single;
        };
        let pages = self.pages_of(spread);
        if pages.len() < 2 {
            return PageSide::Single;
        }
        match pages.iter().position(|p| *p == page) {
            Some(0) => PageSide::Left,
            Some(_) => PageSide::Right,
            None => PageSide::Single,
        }
    }

    /// Add a page to a spread, sized like the one before it.
    pub fn add_page_to(&mut self, spread: SpreadId) -> PageId {
        let bounds = self
            .pages_of(spread)
            .last()
            .and_then(|p| self.pages.get(*p))
            .map_or_else(|| self.first_page_bounds(), |p| p.bounds);

        let page = self.pages.insert(Page {
            bounds,
            layers: Vec::new(),
        });
        if let Some(s) = self.spreads.get_mut(spread) {
            s.pages.push(page);
        }
        self.bump_revision();
        page
    }
```

If `Document` has no `bump_revision` helper, use whatever `add_frame` uses to move `revision`; read it first and match it exactly.

Add `PageSide` and `Page` to the file's imports from `crate::nodes`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p tessera_document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/tessera_document/src/document.rs
git commit -m "Let a page say which side of its spread it is on"
```

---

### Task 3: Resolve the page rectangles

The screen and the PDF must agree about where the trim, the margins and the bleed are. They agree by both reading one computed answer.

**Files:**
- Modify: `crates/tessera_document/src/document.rs`
- Test: inline `#[cfg(test)]` in the same file

**Interfaces:**
- Consumes: task 1 and task 2.
- Produces:
  - `Document::margin_rect(&self, page: PageId) -> Option<DocRect>`
  - `Document::bleed_rect(&self, page: PageId) -> Option<DocRect>`
  - `Document::slug_rect(&self, page: PageId) -> Option<DocRect>`

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn margins_inset_the_page_from_every_edge() {
        let mut doc = Document::new();
        doc.setup.margins = Margins::uniform(36.0);
        let page = doc.page_ids().next().expect("a page");
        let bounds = doc.pages[page].bounds;

        let inner = doc.margin_rect(page).expect("a margin rect");
        assert_eq!(inner.x, bounds.x + 36.0);
        assert_eq!(inner.y, bounds.y + 36.0);
        assert_eq!(inner.width, bounds.width - 72.0);
        assert_eq!(inner.height, bounds.height - 72.0);
    }

    #[test]
    fn the_inside_margin_swaps_sides_between_verso_and_recto() {
        // The whole reason margins are inside/outside rather than left/right.
        let mut doc = Document::new();
        doc.setup.facing_pages = true;
        doc.setup.margins = Margins {
            top: 10.0,
            bottom: 10.0,
            inside: 60.0,
            outside: 20.0,
        };
        let spread = doc.spread_ids().next().expect("a spread");
        let verso = doc.pages_of(spread)[0];
        let recto = doc.add_page_to(spread);

        let v = doc.margin_rect(verso).expect("verso");
        let recto_rect = doc.margin_rect(recto).expect("recto");
        let vb = doc.pages[verso].bounds;
        let rb = doc.pages[recto].bounds;

        // Verso: the spine is on its right, so the wide margin is on the right.
        assert_eq!(v.x, vb.x + 20.0, "verso outside margin is on the left");
        // Recto: the spine is on its left, so the wide margin is on the left.
        assert_eq!(recto_rect.x, rb.x + 60.0, "recto inside margin is on the left");
    }

    #[test]
    fn bleed_grows_outward_rather_than_inward() {
        let mut doc = Document::new();
        doc.setup.bleed = Insets::uniform(9.0);
        let page = doc.page_ids().next().expect("a page");
        let bounds = doc.pages[page].bounds;

        let bleed = doc.bleed_rect(page).expect("a bleed rect");
        assert_eq!(bleed.x, bounds.x - 9.0);
        assert_eq!(bleed.width, bounds.width + 18.0);
    }

    #[test]
    fn the_slug_lies_outside_the_bleed() {
        let mut doc = Document::new();
        doc.setup.bleed = Insets::uniform(9.0);
        doc.setup.slug = Insets::uniform(18.0);
        let page = doc.page_ids().next().expect("a page");

        let bleed = doc.bleed_rect(page).expect("bleed");
        let slug = doc.slug_rect(page).expect("slug");
        assert!(slug.x < bleed.x, "the slug is further out than the bleed");
        assert!(slug.width > bleed.width);
    }

    #[test]
    fn margins_wider_than_the_page_collapse_rather_than_inverting() {
        // An inside-out rectangle would draw as a shape turned inside out and
        // hit-test as nothing. Collapsing to zero is the honest degenerate.
        let mut doc = Document::new();
        doc.setup.margins = Margins::uniform(10_000.0);
        let page = doc.page_ids().next().expect("a page");
        let inner = doc.margin_rect(page).expect("a rect");
        assert!(inner.width >= 0.0 && inner.height >= 0.0);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tessera_document document::`
Expected: FAIL, `no method named margin_rect`.

- [ ] **Step 3: Implement**

```rust
    /// The type area: the page inset by its margins.
    ///
    /// Which physical edge the inside margin falls on depends on the page's
    /// side, which is the whole reason [`Margins`] is not left-and-right.
    pub fn margin_rect(&self, page: PageId) -> Option<DocRect> {
        let bounds = self.pages.get(page)?.bounds;
        let m = self.setup.margins;
        let (left, right) = match self.page_side(page) {
            // A verso's spine is on its right.
            PageSide::Left => (m.outside, m.inside),
            PageSide::Right | PageSide::Single => (m.inside, m.outside),
        };
        Some(shrunk(bounds, m.top, m.bottom, left, right))
    }

    /// The page plus its bleed.
    pub fn bleed_rect(&self, page: PageId) -> Option<DocRect> {
        let b = self.setup.bleed;
        let bounds = self.pages.get(page)?.bounds;
        Some(grown(bounds, b.top, b.bottom, b.left, b.right))
    }

    /// The page plus its bleed plus its slug.
    ///
    /// Measured from the page rather than from the bleed, so that a slug
    /// smaller than the bleed does not read as a negative one; the two are
    /// independent distances from the trim, as they are on a press sheet.
    pub fn slug_rect(&self, page: PageId) -> Option<DocRect> {
        let s = self.setup.slug;
        let bounds = self.pages.get(page)?.bounds;
        Some(grown(bounds, s.top, s.bottom, s.left, s.right))
    }
```

and, as free functions in the same file:

```rust
/// `rect` pulled inward, never past inside-out.
fn shrunk(rect: DocRect, top: f64, bottom: f64, left: f64, right: f64) -> DocRect {
    DocRect {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    }
}

/// `rect` pushed outward.
fn grown(rect: DocRect, top: f64, bottom: f64, left: f64, right: f64) -> DocRect {
    DocRect {
        x: rect.x - left,
        y: rect.y - top,
        width: rect.width + left + right,
        height: rect.height + top + bottom,
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p tessera_document`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/tessera_document/src/document.rs
git commit -m "Resolve the trim, margin, bleed and slug rectangles"
```

---

### Task 4: Guides as document data

**Files:**
- Modify: `crates/tessera_document/src/nodes.rs`, `crates/tessera_document/src/document.rs`
- Test: inline `#[cfg(test)]` in both

**Interfaces:**
- Produces:
  - `Axis::{ Horizontal, Vertical }` — `Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`
  - `Guide { axis: Axis, position: f64, locked: bool }` — same derives plus `Default` on `locked`
  - `Spread::guides: Vec<Guide>` — `#[serde(default)]`
  - `Document::add_guide(&mut self, spread: SpreadId, guide: Guide)`
  - `Document::remove_guide(&mut self, spread: SpreadId, index: usize) -> Option<Guide>`
  - `Document::guides_of(&self, spread: SpreadId) -> &[Guide]`

Guides belong to the spread, not the page. InDesign has both, and the difference matters only when pages within a spread move independently — which is milestone 3's problem. One kind now, and a note saying so, beats two kinds guessed at.

- [ ] **Step 1: Write the failing tests**

In `crates/tessera_document/src/document.rs`'s test module:

```rust
    #[test]
    fn a_guide_added_to_a_spread_can_be_read_back() {
        let mut doc = Document::new();
        let spread = doc.spread_ids().next().expect("a spread");
        doc.add_guide(
            spread,
            Guide {
                axis: Axis::Vertical,
                position: 120.0,
                locked: false,
            },
        );

        let guides = doc.guides_of(spread);
        assert_eq!(guides.len(), 1);
        assert_eq!(guides[0].position, 120.0);
        assert_eq!(guides[0].axis, Axis::Vertical);
    }

    #[test]
    fn adding_a_guide_moves_the_revision_so_the_canvas_redraws() {
        let mut doc = Document::new();
        let spread = doc.spread_ids().next().expect("a spread");
        let before = doc.revision();
        doc.add_guide(
            spread,
            Guide {
                axis: Axis::Horizontal,
                position: 40.0,
                locked: false,
            },
        );
        assert_ne!(doc.revision(), before);
    }

    #[test]
    fn removing_a_guide_returns_it_and_leaves_the_rest() {
        let mut doc = Document::new();
        let spread = doc.spread_ids().next().expect("a spread");
        for position in [10.0, 20.0, 30.0] {
            doc.add_guide(
                spread,
                Guide {
                    axis: Axis::Vertical,
                    position,
                    locked: false,
                },
            );
        }
        let removed = doc.remove_guide(spread, 1).expect("the middle one");
        assert_eq!(removed.position, 20.0);
        assert_eq!(
            doc.guides_of(spread)
                .iter()
                .map(|g| g.position)
                .collect::<Vec<_>>(),
            vec![10.0, 30.0]
        );
    }

    #[test]
    fn removing_a_guide_that_is_not_there_returns_nothing() {
        let mut doc = Document::new();
        let spread = doc.spread_ids().next().expect("a spread");
        assert!(doc.remove_guide(spread, 7).is_none());
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tessera_document document::`
Expected: FAIL, `cannot find type Guide in this scope`.

- [ ] **Step 3: Implement**

In `crates/tessera_document/src/nodes.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// A ruler guide, in spread coordinates.
///
/// On the spread rather than on a page: the two differ only once pages within
/// a spread can move independently, which is milestone 3's concern. One kind
/// now beats two kinds guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Guide {
    pub axis: Axis,
    /// Where it sits along the axis it cuts across: an `x` for a vertical
    /// guide, a `y` for a horizontal one.
    pub position: f64,
    pub locked: bool,
}
```

and extend `Spread`:

```rust
pub struct Spread {
    pub pages: Vec<PageId>,
    /// `serde(default)` so a document written before guides existed loads
    /// with none, which is the truth about it.
    #[serde(default)]
    pub guides: Vec<Guide>,
}
```

Fix every `Spread { pages: ... }` literal the compiler now names — `Document::new` at minimum — by adding `guides: Vec::new()`.

In `crates/tessera_document/src/document.rs`:

```rust
    pub fn guides_of(&self, spread: SpreadId) -> &[Guide] {
        self.spreads
            .get(spread)
            .map_or(&[], |s| s.guides.as_slice())
    }

    pub fn add_guide(&mut self, spread: SpreadId, guide: Guide) {
        if let Some(s) = self.spreads.get_mut(spread) {
            s.guides.push(guide);
            self.bump_revision();
        }
    }

    pub fn remove_guide(&mut self, spread: SpreadId, index: usize) -> Option<Guide> {
        let s = self.spreads.get_mut(spread)?;
        if index >= s.guides.len() {
            return None;
        }
        let removed = s.guides.remove(index);
        self.bump_revision();
        Some(removed)
    }
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p tessera_document`
Expected: PASS, including round-trip property tests — guides are part of the document now and must survive a save.

- [ ] **Step 5: Commit**

```bash
git add crates/tessera_document/src/nodes.rs crates/tessera_document/src/document.rs
git commit -m "Make a guide part of the document rather than of the view"
```

---

### Task 5: Format version 5, with its migration test

**This is the only task that touches `FORMAT_VERSION`.**

**Files:**
- Modify: `crates/tessera_document/src/format/mod.rs`
- Test: `crates/tessera_document/tests/round_trip.rs`

**Interfaces:**
- Produces: `FORMAT_VERSION == 5`.

- [ ] **Step 1: Write the failing test**

Append to `crates/tessera_document/tests/round_trip.rs`:

```rust
#[test]
fn a_version_four_document_still_opens_and_gains_empty_page_setup() {
    // Everything phase B added carries serde(default), so a version-4
    // document needs no rewriting — but "needs no rewriting" is a claim, and
    // this is the test that makes it one that can fail.
    let dir = std::env::temp_dir();
    let path = dir.join("tessera-v4-migration.tessera");
    let _ = std::fs::remove_file(&path);

    let mut original = Document::new();
    let layer = original.default_layer().expect("a layer");
    original.add_frame(
        layer,
        Frame {
            bounds: DocRect {
                x: 12.0,
                y: 34.0,
                width: 56.0,
                height: 78.0,
            },
            transform: Transform::IDENTITY,
            kind: FrameKind::Rectangle,
            fill: Color::BLACK,
            stroke: None,
        },
    );

    format::save(&original, &path).expect("save");
    format::rewrite_version_for_test(&path, 4).expect("stamp it as version 4");

    let reopened = format::load(&path).expect("a version-4 document still opens");
    assert_eq!(reopened.frames.len(), 1, "its frames survived");
    assert_eq!(
        reopened.setup,
        tessera_document::nodes::DocumentSetup::default(),
        "and it gained no margins it never had"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_document_from_a_newer_build_is_refused_rather_than_guessed_at() {
    let path = std::env::temp_dir().join("tessera-v99-refusal.tessera");
    let _ = std::fs::remove_file(&path);

    format::save(&Document::new(), &path).expect("save");
    format::rewrite_version_for_test(&path, 99).expect("stamp");

    let error = format::load(&path).expect_err("a newer format must be refused");
    assert!(
        matches!(error, format::FormatError::NewerFormat { found: 99, .. }),
        "got {error}"
    );

    let _ = std::fs::remove_file(&path);
}
```

Match the imports the file already uses; add any the compiler names.

- [ ] **Step 2: Run to verify the first test fails**

Run: `cargo test -p tessera_document --test round_trip`
Expected: FAIL — `no field setup` until task 1 landed, or the version assertion, depending on order. If both pass already, the version has not been bumped yet, which step 3 fixes.

- [ ] **Step 3: Bump the version and record the step**

In `crates/tessera_document/src/format/mod.rs`:

```rust
pub const FORMAT_VERSION: u32 = 5;
```

and add to `migrate`, below the 3→4 note:

```rust
    // 4 -> 5: the document gained page setup — margins, bleed, slug and
    // whether pages face — and a spread gained guides.
    //
    // Nothing to rewrite. Every field carries `serde(default)`, and each
    // default is the truth about a document that never had the field: no
    // margins, no bleed, no slug, pages that do not face, no guides. A
    // fabricated default — 10mm margins, say — would be inventing a decision
    // the user never made.
    //
    // The version moves anyway, so a build without page setup refuses a
    // document that uses it rather than silently dropping it on the next
    // save. That is the same reason 3 -> 4 moved.
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p tessera_document`
Expected: PASS, all of it.

- [ ] **Step 5: Commit**

```bash
git add crates/tessera_document/src/format/mod.rs crates/tessera_document/tests/round_trip.rs
git commit -m "Move the format to version 5, once, for the whole page"
```

---

### Task 6: Draw the spread

**Files:**
- Modify: `crates/tessera_layout/src/resolve.rs`, `crates/tessera_render/src/scene.rs`, `crates/tessera_ui/src/view/viewport.rs`
- Test: inline in `resolve.rs`; existing scene tests must keep passing

**Interfaces:**
- Produces:
  - `ResolvedPage { bounds: DocRect, margins: DocRect, bleed: DocRect, slug: DocRect }` — `Debug, Clone, Copy, PartialEq`
  - `ResolvedDocument::pages: Vec<ResolvedPage>`
  - `build_scene(resolved: &ResolvedDocument, view: ViewTransform)` — the `page: DocRect` parameter **goes away**; the pages come from `resolved`

Dropping the parameter is the point. While the caller passes a page rectangle separately, the screen and the PDF each decide for themselves where the page is, and one of them will eventually be wrong.

- [ ] **Step 1: Write the failing test**

In `crates/tessera_layout/src/resolve.rs`'s test module:

```rust
    #[test]
    fn resolving_carries_every_pages_rectangles() {
        let mut doc = Document::new();
        doc.setup.margins = tessera_document::nodes::Margins::uniform(36.0);
        doc.setup.bleed = tessera_document::nodes::Insets::uniform(9.0);

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        assert_eq!(resolved.pages.len(), doc.page_ids().count());
        let page = resolved.pages[0];
        assert_eq!(page.margins.width, page.bounds.width - 72.0);
        assert_eq!(page.bleed.width, page.bounds.width + 18.0);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p tessera_layout`
Expected: FAIL, `no field pages on ResolvedDocument`.

- [ ] **Step 3: Carry the pages through resolve**

In `crates/tessera_layout/src/resolve.rs`:

```rust
/// One page, with the rectangles that describe it.
///
/// Computed once here so the screen and the PDF cannot disagree about where
/// the trim is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPage {
    pub bounds: DocRect,
    pub margins: DocRect,
    pub bleed: DocRect,
    pub slug: DocRect,
}
```

Add `pub pages: Vec<ResolvedPage>` to `ResolvedDocument`, and populate it in `resolve` before the frame loop:

```rust
    let pages = doc
        .page_ids()
        .filter_map(|id| {
            Some(ResolvedPage {
                bounds: doc.pages.get(id)?.bounds,
                margins: doc.margin_rect(id)?,
                bleed: doc.bleed_rect(id)?,
                slug: doc.slug_rect(id)?,
            })
        })
        .collect();
```

and return it in the `ResolvedDocument` literal. Export `ResolvedPage` from `crates/tessera_layout/src/lib.rs` alongside `ResolvedDocument`.

- [ ] **Step 4: Draw them**

In `crates/tessera_render/src/scene.rs`, change `build_scene` to take `(resolved: &ResolvedDocument, view: ViewTransform)` and, where it currently fills the single `page` rectangle, loop over `resolved.pages`:

- fill each `bounds` with the page colour, as now;
- stroke each `bleed` with a hairline in a distinct colour, only when it differs from `bounds`;
- stroke each `margins` with a hairline, only when it differs from `bounds`.

Reuse the existing `hairline(view)` helper so these behave like every other thin rule at low zoom. Colours come from constants in this file beside the existing page colour — the render crate has no access to `tessera_ui::theme`, and guides drawn into the Vello scene are document furniture rather than interface chrome.

**Do not draw the slug yet.** It has no visual distinct from the bleed until screen modes exist in phase C, and drawing two identical rectangles teaches the reader nothing.

- [ ] **Step 5: Fix the callers**

Run: `cargo check -p tessera_render -p tessera_ui -p tessera_pdf --all-targets`

Update `viewport.rs` to call `build_scene(resolved, view)`, dropping the `page` argument it used to compute. Leave `state.first_page_bounds()` where it is still used for zoom-to-fit.

- [ ] **Step 6: Run everything**

Run: `cargo test -p tessera_layout -p tessera_render -p tessera_ui`
Expected: PASS. GPU tests are excluded; `--test gpu_render` runs separately.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Draw the page's own rectangles from one resolved answer"
```

---

### Task 7: The document setup inspector

The first visible result of this phase.

**Files:**
- Modify: `crates/tessera_ui/src/view/panels.rs`, `crates/tessera_ui/src/command.rs`
- Test: inline `#[cfg(test)]` in `command.rs`

**Interfaces:**
- Produces:
  - `Command::SetDocumentSetup(DocumentSetup)`
  - `panels::document_setup(ui, state)` — the inspector's no-selection body

- [ ] **Step 1: Write the failing test**

In `crates/tessera_ui/src/command.rs`'s test module:

```rust
    #[test]
    fn setting_the_page_setup_is_one_undoable_step() {
        let mut state = TesseraApp::headless();
        let before = state.active().document().setup;

        let wanted = tessera_document::nodes::DocumentSetup {
            margins: tessera_document::nodes::Margins::uniform(36.0),
            bleed: tessera_document::nodes::Insets::uniform(9.0),
            slug: tessera_document::nodes::Insets::default(),
            facing_pages: true,
        };
        apply(&mut state, Command::SetDocumentSetup(wanted));
        assert_eq!(state.active().document().setup, wanted);

        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().setup,
            before,
            "one undo puts the whole setup back"
        );
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p tessera_ui command::`
Expected: FAIL, `no variant named SetDocumentSetup`.

- [ ] **Step 3: Add the command**

In `crates/tessera_ui/src/command.rs`, add the variant to `Command`, make sure `mutates()` returns true for it, and handle it:

```rust
        Command::SetDocumentSetup(setup) => {
            state.active_mut().document_mut().setup = setup;
        }
```

Setting the whole struct at once, rather than one command per field, is what makes a page-setup edit a single undo entry instead of four.

- [ ] **Step 4: Build the panel**

In `crates/tessera_ui/src/view/panels.rs`, replace the inspector's `"No Selection"` branch with a `document_setup` body holding, in this fixed order:

- **Page** — width and height, editing `Page.bounds` of every page through a command;
- **Facing pages** — a checkbox;
- **Margins** — top, bottom, inside, outside, relabelled *left* and *right* when facing pages is off, because that is what they then mean;
- **Bleed** — top, bottom, left, right;
- **Slug** — the same.

Every field parses through `tessera_geometry::Unit::parse_to_points` with the preference's unit, and displays through `Unit::format`. That is what phase A's task 1 was for; the first consumer arrives here.

Reuse the existing `scrub` helper for drag-to-scrub, and emit **one** `Command::SetDocumentSetup` per completed edit, never per frame — otherwise a drag fills the undo stack, which is the failure `undo-bracketed` exists to prevent.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p tessera_ui`
Expected: PASS.

- [ ] **Step 6: Look at it**

Run: `cargo run --release -p tessera_app`

With nothing selected, the inspector shows Page, Facing pages, Margins, Bleed and Slug. Set margins to 36 pt and see the guide appear inside the page. Type `20mm` into a bleed field and watch it convert. Toggle facing pages. Save, quit, reopen, and find all of it as it was.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Give the document a setup panel"
```

---

### Task 8: The PDF's trim and bleed boxes

Exporting a document that has a bleed without recording it discards the user's intent silently, which the cross-cutting rules forbid.

**Files:**
- Modify: `crates/tessera_pdf/src/writer.rs`, `crates/tessera_ui/src/file_ops.rs`
- Test: `crates/tessera_pdf/tests/export.rs`

**Interfaces:**
- `tessera_pdf::export(resolved: &ResolvedDocument, page: DocRect)` becomes `export(resolved: &ResolvedDocument)`, reading the page from `resolved.pages` as task 6's `build_scene` does.

- [ ] **Step 1: Write the failing test**

Append to `crates/tessera_pdf/tests/export.rs`:

```rust
#[test]
fn a_document_with_a_bleed_records_a_trim_box_and_a_bleed_box() {
    let mut doc = tessera_document::document::Document::new();
    doc.setup.bleed = tessera_document::nodes::Insets::uniform(9.0);

    let mut shaper = tessera_text::shape::Shaper::new();
    let resolved = tessera_layout::resolve(&doc, &mut shaper);
    let bytes = tessera_pdf::export(&resolved).expect("export");
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("/TrimBox"), "the trim must be recorded");
    assert!(text.contains("/BleedBox"), "and so must the bleed");
}

#[test]
fn a_document_without_a_bleed_still_exports() {
    let doc = tessera_document::document::Document::new();
    let mut shaper = tessera_text::shape::Shaper::new();
    let resolved = tessera_layout::resolve(&doc, &mut shaper);
    assert!(tessera_pdf::export(&resolved).is_ok());
}
```

If `pdf-writer` compresses streams so the box names are not findable as plain text, assert instead by re-parsing with whatever the existing tests in this file already use to inspect output — read them first and follow that pattern rather than introducing a second one.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p tessera_pdf`
Expected: FAIL — `export` takes two arguments.

- [ ] **Step 3: Implement**

In `crates/tessera_pdf/src/writer.rs`, take the first `ResolvedPage` from `resolved.pages` instead of a `page: DocRect` parameter, and on the page object set:

- `media_box` to the **bleed** rectangle when a bleed exists, otherwise the page — the media box must contain everything that gets imaged;
- `trim_box` to the page rectangle, always;
- `bleed_box` to the bleed rectangle, always (equal to the trim when there is no bleed).

All four are in PDF user space with the origin at the bottom left, so they go through the existing `to_pdf_y` conversion. The content stream's coordinate origin must stay the **trim** corner, or every object on the page shifts by the bleed when a bleed is set — write a test asserting an object's position is unchanged by adding a bleed if that is not already covered.

- [ ] **Step 4: Fix the caller**

In `crates/tessera_ui/src/file_ops.rs`, `export_pdf_to_path` drops its `state.first_page_bounds()` argument.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p tessera_pdf -p tessera_ui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Record the trim and the bleed in the exported PDF"
```

---

## Closing the phase

- [ ] **Run the full non-GPU suite**

```bash
cargo test -p tessera_geometry -p tessera_color -p tessera_io -p tessera_text -p tessera_document -p tessera_layout -p tessera_pdf -p tessera_ui
```

- [ ] **Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Perform the phase's sentence, by hand, in the running application**

Set a page size, margins, a bleed and a slug. Turn on facing pages and watch the spread appear side by side with the inside margins swapping between the left page and the right. Save, quit, reopen, and find every one of those numbers as they were left. Export a PDF and confirm the trim and bleed boxes are in it.

- [ ] **Tick phase B in the roadmap**, and record what is still owed.

- [ ] **Write the phase C plan** — the interface, on the foundations phases A and B now provide.
