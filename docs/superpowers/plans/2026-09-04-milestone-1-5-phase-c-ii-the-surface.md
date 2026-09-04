# Milestone 1.5 Phase C-ii — The Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the canvas itself an instrument — objects that align to each other and to the page, spatial verbs beside the selection, rulers that yield guides, and screen modes that show the document as it will print.

**Architecture:** Alignment is pure geometry over `Document::visual_bounds`, so it is testable without a window and reads the same rectangles the renderer draws. The canvas toolbar carries only verbs whose effect is spatial (D2); every value stays in the rail. Guides are already document data from phase B, so the ruler only has to create and move them.

**Tech Stack:** Rust 2024, egui 0.35, vello, kurbo. No new dependencies.

**Spec:** [`docs/superpowers/specs/2026-09-03-instrument-milestone-design.md`](../specs/2026-09-03-instrument-milestone-design.md) — D2 above all.

**Preceded by:** [phase C-i, the rail](2026-09-04-milestone-1-5-phase-c-i-the-rail.md).

## Global Constraints

- `unsafe_code = "forbid"`. Document units are **points**.
- **No format version bump.** Guides already exist in the model (phase B); screen mode and ruler origin are view state.
- Every mutation goes through `Command`, or carries an `undo-bracketed:` marker.
- One completed gesture is **one** undo entry.
- Tests land in the same commit. `cargo clippy --workspace --all-targets -- -D warnings` clean before each.
- Single-crate test commands (`-p`), so GPU tests never join the run.

---

### Task 1: Align and distribute

**Files:**
- Create: `crates/tessera_ui/src/align.rs`
- Modify: `crates/tessera_ui/src/lib.rs`, `crates/tessera_ui/src/command.rs`
- Test: inline in `align.rs`, plus command tests

**Interfaces:**
- Consumes: `Document::visual_bounds(FrameId) -> Option<DocRect>`, `Document::margin_rect`, `Document::bleed_rect`, `Selection`.
- Produces:
  - `Edge::{ Left, HCentre, Right, Top, VCentre, Bottom }`
  - `AlignTo::{ Selection, Margins, Page, Spread }`
  - `align_deltas(bounds: &[DocRect], target: DocRect, edge: Edge) -> Vec<(f64, f64)>`
  - `distribute_deltas(bounds: &[DocRect], axis: Axis) -> Vec<(f64, f64)>`
  - `Command::Align { edge: Edge, to: AlignTo }`
  - `Command::Distribute(Axis)`

Pure functions over rectangles, so the arithmetic is testable without a document, a selection or a window — which is what makes the awkward cases (two objects, coincident objects) cheap to pin.

- [ ] **Step 1: Write the failing tests**

Create `crates/tessera_ui/src/align.rs` holding only:

```rust
//! Aligning and distributing, as arithmetic over rectangles.

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::Axis;
    use tessera_geometry::DocRect;

    fn r(x: f64, y: f64, w: f64, h: f64) -> DocRect {
        DocRect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn bounding(rects: &[DocRect]) -> DocRect {
        super::bounding_box(rects).expect("some rectangles")
    }

    #[test]
    fn aligning_left_moves_every_edge_to_the_leftmost() {
        let rects = [r(10.0, 0.0, 20.0, 10.0), r(50.0, 0.0, 20.0, 10.0)];
        let deltas = align_deltas(&rects, bounding(&rects), Edge::Left);
        assert_eq!(deltas[0], (0.0, 0.0), "the leftmost does not move");
        assert_eq!(deltas[1], (-40.0, 0.0));
    }

    #[test]
    fn aligning_centres_uses_the_targets_middle() {
        let rects = [r(0.0, 0.0, 10.0, 10.0), r(100.0, 0.0, 30.0, 10.0)];
        let target = bounding(&rects);
        let deltas = align_deltas(&rects, target, Edge::HCentre);
        let middle = target.x + target.width / 2.0;
        assert!((rects[0].x + deltas[0].0 + 5.0 - middle).abs() < 1e-9);
        assert!((rects[1].x + deltas[1].0 + 15.0 - middle).abs() < 1e-9);
    }

    #[test]
    fn aligning_to_a_page_ignores_where_the_selection_happens_to_be() {
        // Aligning to the page must not depend on the objects' own extent,
        // which is the whole difference between the two targets.
        let rects = [r(500.0, 0.0, 20.0, 10.0)];
        let page = r(0.0, 0.0, 612.0, 792.0);
        let deltas = align_deltas(&rects, page, Edge::Left);
        assert_eq!(deltas[0], (-500.0, 0.0));
    }

    #[test]
    fn aligning_vertically_leaves_x_alone() {
        let rects = [r(10.0, 5.0, 20.0, 10.0), r(50.0, 90.0, 20.0, 10.0)];
        let deltas = align_deltas(&rects, bounding(&rects), Edge::Top);
        assert!(deltas.iter().all(|d| d.0 == 0.0), "a vertical align moved x");
    }

    #[test]
    fn distributing_spaces_the_centres_evenly() {
        // Three boxes, the middle one off-centre. Distributing must put its
        // centre exactly halfway between the outer two.
        let rects = [
            r(0.0, 0.0, 10.0, 10.0),
            r(20.0, 0.0, 10.0, 10.0),
            r(100.0, 0.0, 10.0, 10.0),
        ];
        let deltas = distribute_deltas(&rects, Axis::Horizontal);
        assert_eq!(deltas[0], (0.0, 0.0), "the outermost do not move");
        assert_eq!(deltas[2], (0.0, 0.0));

        let centre = |i: usize| rects[i].x + deltas[i].0 + rects[i].width / 2.0;
        assert!((centre(1) - (centre(0) + centre(2)) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn distributing_fewer_than_three_does_nothing() {
        // With two objects there is nothing between them to space out, and
        // moving either would be a surprise.
        let rects = [r(0.0, 0.0, 10.0, 10.0), r(50.0, 0.0, 10.0, 10.0)];
        let deltas = distribute_deltas(&rects, Axis::Horizontal);
        assert!(deltas.iter().all(|d| *d == (0.0, 0.0)));
    }

    #[test]
    fn distributing_coincident_objects_does_not_divide_by_zero() {
        let rects = [
            r(0.0, 0.0, 10.0, 10.0),
            r(0.0, 0.0, 10.0, 10.0),
            r(0.0, 0.0, 10.0, 10.0),
        ];
        let deltas = distribute_deltas(&rects, Axis::Horizontal);
        assert!(deltas.iter().all(|d| d.0.is_finite()));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tessera_ui align::`
Expected: FAIL to compile, `cannot find function align_deltas`.

- [ ] **Step 3: Implement**

Write `bounding_box`, `align_deltas` and `distribute_deltas` above the test module. Sort by centre before distributing, and return deltas in the **input order** so the caller can zip them against the selection — returning them sorted would silently move the wrong objects.

- [ ] **Step 4: Add the commands**

`Command::Align { edge, to }` resolves the target rectangle — the selection's bounding box, or the active page's margin, trim or spread rectangle — then translates every selected frame by its delta, in one command so the whole alignment is one undo entry.

`Command::Distribute(axis)` does the same with `distribute_deltas`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p tessera_ui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Align and distribute, as arithmetic over rectangles"
```

---

### Task 2: The canvas toolbar

**Files:**
- Create: `crates/tessera_ui/src/view/canvas_toolbar.rs`
- Modify: `crates/tessera_ui/src/view/mod.rs`, `crates/tessera_ui/src/view/viewport.rs`, `crates/tessera_ui/src/icons.rs`
- Test: inline placement test

**Interfaces:**
- Produces:
  - `canvas_toolbar::show(ui, state, selection_screen_rect: Rect)`
  - `canvas_toolbar::place(selection: Rect, toolbar: Vec2, viewport: Rect) -> Pos2`

This is D2. The toolbar carries **only spatial verbs** — align left/centre/right, distribute, flip horizontal, flip vertical, rotate 90° each way. No values. Nothing here appears in the rail, so the two surfaces cannot disagree.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn the_toolbar_goes_below_the_selection_when_there_is_room() {
        let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let selection = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 100.0));
        let at = place(selection, vec2(180.0, 28.0), viewport);
        assert!(at.y > selection.max.y, "it sat over the object");
    }

    #[test]
    fn the_toolbar_goes_above_when_there_is_no_room_below() {
        let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let selection = Rect::from_min_size(pos2(100.0, 540.0), vec2(200.0, 50.0));
        let at = place(selection, vec2(180.0, 28.0), viewport);
        assert!(at.y < selection.min.y, "it fell off the bottom");
    }

    #[test]
    fn the_toolbar_stays_inside_the_viewport_horizontally() {
        let viewport = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let selection = Rect::from_min_size(pos2(760.0, 100.0), vec2(30.0, 30.0));
        let at = place(selection, vec2(180.0, 28.0), viewport);
        assert!(at.x >= viewport.min.x);
        assert!(at.x + 180.0 <= viewport.max.x, "it ran off the right edge");
    }
```

- [ ] **Step 2: Run to verify it fails**, then implement `place` and the toolbar.

Placement rule: below the selection when there is room, above otherwise, clamped horizontally to the viewport. **Suppressed entirely in Preview and the other printing screen modes** — R4 in the spec.

- [ ] **Step 3: Add the icons**

Extend `icons.rs` with the Lucide glyphs the toolbar needs: `align-left`, `align-center-horizontal`, `align-right`, `align-start-vertical`, `align-center-vertical`, `align-end-vertical`, `flip-horizontal`, `flip-vertical`, `rotate-cw`, `rotate-ccw`. Add each to `ALL` so `every_icon_parses` covers it.

- [ ] **Step 4: Run the tests and commit**

```bash
git add -A
git commit -m "Put the spatial verbs beside the object they act on"
```

---

### Task 3: Rulers

**Files:**
- Create: `crates/tessera_ui/src/view/rulers.rs`
- Modify: `crates/tessera_ui/src/view/mod.rs`
- Test: inline tick-spacing tests

**Interfaces:**
- Produces:
  - `rulers::tick_spacing(unit: Unit, zoom: f64) -> f64` — in points
  - `rulers::show_horizontal(ui, state, canvas: Rect)` and `show_vertical`

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn ticks_never_get_closer_than_a_readable_gap() {
        for zoom in [0.05, 0.25, 1.0, 4.0, 32.0] {
            for unit in Unit::ALL {
                let step = tick_spacing(unit, zoom);
                assert!(step > 0.0, "{unit:?} at {zoom} gave {step}");
                assert!(
                    step * zoom >= 4.0,
                    "{unit:?} at {zoom} would draw ticks {} px apart",
                    step * zoom
                );
            }
        }
    }

    #[test]
    fn zooming_in_subdivides_rather_than_multiplying() {
        // More zoom must never mean a coarser ruler.
        let coarse = tick_spacing(Unit::Millimetres, 0.25);
        let fine = tick_spacing(Unit::Millimetres, 4.0);
        assert!(fine <= coarse);
    }
```

- [ ] **Step 2: Implement**, choosing from a 1–2–5 ladder of multiples of the unit so that the labels are round numbers a person reads rather than whatever fits.

The zero point sits at the active spread's top-left corner. A unit selector at the rulers' intersection writes `state.prefs.unit` **and saves the preferences**, so the choice survives a restart — that is what phase A's store is for.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "Add rulers that read in the unit the user chose"
```

---

### Task 4: Guides you can drag out

**Files:**
- Modify: `crates/tessera_ui/src/view/rulers.rs`, `crates/tessera_ui/src/view/viewport.rs`, `crates/tessera_ui/src/command.rs`
- Test: command tests

**Interfaces:**
- Consumes: `Document::add_guide`, `remove_guide`, `guides_of` — all from phase B.
- Produces:
  - `Command::AddGuide { spread: SpreadId, guide: Guide }`
  - `Command::MoveGuide { spread: SpreadId, index: usize, position: f64 }`
  - `Command::RemoveGuide { spread: SpreadId, index: usize }`

- [ ] **Step 1: Write the failing tests** — adding a guide is one undo entry; moving one is one entry for the whole drag, not one per pointer move; dragging a guide back onto its ruler removes it.

- [ ] **Step 2: Implement.** The drag is a bracketed gesture: record history on press, preview live, and apply one `MoveGuide` on release. Mark the live write `undo-bracketed:` — `tests/command_invariant.rs` will fail otherwise, which is the point of it.

- [ ] **Step 3: Draw them** in the viewport, as cyan hairlines, under the objects and above the page. Hidden in the printing screen modes.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "Let a guide be dragged off the ruler and back onto it"
```

---

### Task 5: Screen modes

**Files:**
- Modify: `crates/tessera_ui/src/app.rs`, `crates/tessera_ui/src/view/mod.rs`, `crates/tessera_ui/src/view/viewport.rs`, `crates/tessera_render/src/scene.rs`, `crates/tessera_ui/src/theme.rs`
- Test: inline in `app.rs` and `scene.rs`

**Interfaces:**
- Produces:
  - `ScreenMode::{ Normal, Preview, Bleed, Slug }`
  - `ScreenMode::shows_chrome(self) -> bool`
  - `ScreenMode::revealed(self, page: &ResolvedPage) -> DocRect`
  - `TesseraApp::screen_mode: ScreenMode`

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn only_normal_shows_the_interface_furniture() {
        assert!(ScreenMode::Normal.shows_chrome());
        for mode in [ScreenMode::Preview, ScreenMode::Bleed, ScreenMode::Slug] {
            assert!(!mode.shows_chrome(), "{mode:?} showed guides and handles");
        }
    }

    #[test]
    fn each_printing_mode_reveals_more_than_the_last() {
        let page = /* a ResolvedPage with a 9pt bleed and an 18pt slug */;
        let preview = ScreenMode::Preview.revealed(&page).width;
        let bleed = ScreenMode::Bleed.revealed(&page).width;
        let slug = ScreenMode::Slug.revealed(&page).width;
        assert!(preview < bleed, "bleed must show more than preview");
        assert!(bleed < slug, "slug must show more than bleed");
    }
```

- [ ] **Step 2: Implement.** `W` cycles Normal → Preview and back; the four are also reachable from the View menu. In any mode but Normal, handles, frame edges, guides, margin and bleed rules, rulers and the canvas toolbar all go, and the pasteboard is painted the **fixed neutral grey** of D8 — in both themes, because perceived colour shifts with its surround and a designer judging an ink must not be judging it against two different backgrounds.

The document is clipped to `revealed`, so Preview shows the trim exactly as it will print.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "Add the screen modes, and hold the surround constant in them"
```

---

## Closing the plan

- [ ] Full non-GPU suite; clippy at `-D warnings`.
- [ ] **Perform the sentence, by hand:**

> Select three objects and align their left edges from the toolbar beside them. Distribute them. Align them to the page margin instead. Drag a guide off the ruler and drop it on the page; drag it back onto the ruler and watch it go. Switch the ruler to picas and watch every field follow. Press `W`: the handles, guides, rules and rulers go and the page sits on a neutral grey. Press it again and get them back.

- [ ] Tick C6–C9, recording anything partial.
- [ ] Write the **C-iii** plan: status bar, command palette, menus, icon set.
