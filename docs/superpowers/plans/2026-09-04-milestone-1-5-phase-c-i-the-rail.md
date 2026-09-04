# Milestone 1.5 Phase C-i — The Rail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the inspector the whole of an object's properties — reference point, position, size, scale, rotation, shear, fill and the entire stroke model — so that capability which already ships stops being unreachable.

**Architecture:** Every numeric transform field reads the frame's transform through phase A's `Decomposition` and writes it back as a composition about phase A's `Anchor`, so the fields, the handles and the reference point all mean one thing. Sections sit in a fixed order chosen so that the ones that can be absent are last — which is how a section hides without moving any other.

**Tech Stack:** Rust 2024, egui 0.35, kurbo. No new dependencies.

**Spec:** [`docs/superpowers/specs/2026-09-03-instrument-milestone-design.md`](../specs/2026-09-03-instrument-milestone-design.md) — D1, D4, D6.

## Scope: phase C is three plans, not one

Phase C is thirteen roadmap items across three subsystems that each produce working software alone. This plan is the first:

| Plan | Roadmap items | What it is |
|---|---|---|
| **C-i — The Rail** (this one) | C1–C5 | The inspector: reference point, transform fields, fill, the stroke model |
| **C-ii — The Surface** | C6–C9 | Align and distribute, the canvas toolbar, rulers and guides, screen modes |
| **C-iii — The Chrome** | C10–C13 | Status bar, command palette, menus, the icon set |

C-ii and C-iii get their own plans, written when this one has landed and its interfaces are real rather than predicted.

## Global Constraints

- `unsafe_code = "forbid"` at the workspace level.
- Document units are **points**. Fields convert through `Unit` at the edge only.
- **No format version bump.** Everything here is view state, an application preference, or a surface for a model field that already ships. If an item needs a field in `nodes.rs`, it is not in this plan.
- Every mutation goes through `Command`, or carries an `undo-bracketed:` marker. `tests/command_invariant.rs` enforces it.
- One completed edit is **one** undo entry. A drag-scrub must not push an entry per frame.
- Tests land in the same commit as the code.
- `cargo clippy --workspace --all-targets -- -D warnings` clean before each commit.
- Single-crate test commands (`-p`), so GPU tests never join the run.

## A correction to the spec, made before starting

D1 says a section that does not apply "is hidden, and the sections that remain **do not move up to fill the gap**." As written that is impossible: hiding something in a vertical stack moves everything below it.

What makes it true is the **order**, not the hiding. Sections run most-universal first — **Transform, Fill, Stroke, Text, Frame** — so the ones that can be absent are always last, and hiding them never moves anything a user reaches for often. Transform, Fill and Stroke apply to every frame and therefore never move at all.

That is what D1 was reaching for and this plan implements. The spec is amended in task 1.

---

### Task 1: The inspector shell

**Files:**
- Modify: `crates/tessera_ui/src/view/panels.rs`
- Modify: `docs/superpowers/specs/2026-09-03-instrument-milestone-design.md`
- Test: inline `#[cfg(test)]` in `panels.rs`

**Interfaces:**
- Produces:
  - `Section::{ Transform, Fill, Stroke, Text, Frame }` — `Debug, Clone, Copy, PartialEq, Eq`
  - `Section::ALL: [Section; 5]` — in display order
  - `Section::applies_to(self, frame: &Frame) -> bool`
  - `Section::title(self) -> &'static str`

- [ ] **Step 1: Write the failing test**

Append to `crates/tessera_ui/src/view/panels.rs`, creating a `#[cfg(test)] mod tests` if there is none:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::{Frame, FrameKind};
    use tessera_geometry::{DocRect, Transform};

    fn rect_frame() -> Frame {
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            transform: Transform::IDENTITY,
            kind: FrameKind::Rectangle,
            fill: Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn the_sections_that_can_be_absent_come_last() {
        // This is what makes D1 true. Hiding a section moves everything below
        // it, so the ones that apply to every frame must be above the ones
        // that do not — then hiding never moves anything reached for often.
        let frame = rect_frame();
        let last_universal = Section::ALL
            .iter()
            .rposition(|s| s.applies_to(&frame))
            .expect("some section applies");
        let first_absent = Section::ALL.iter().position(|s| !s.applies_to(&frame));

        if let Some(first_absent) = first_absent {
            assert!(
                first_absent > last_universal,
                "an absent section sits above a present one, so hiding it \
                 would move the present one"
            );
        }
    }

    #[test]
    fn transform_fill_and_stroke_apply_to_every_frame() {
        let frame = rect_frame();
        for section in [Section::Transform, Section::Fill, Section::Stroke] {
            assert!(section.applies_to(&frame), "{section:?} must never move");
        }
    }

    #[test]
    fn the_text_section_belongs_only_to_a_text_frame() {
        assert!(!Section::Text.applies_to(&rect_frame()));
    }

    #[test]
    fn every_section_has_a_title() {
        for section in Section::ALL {
            assert!(!section.title().is_empty(), "{section:?} has no title");
        }
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p tessera_ui panels::`
Expected: FAIL to compile, `cannot find type Section in this scope`.

- [ ] **Step 3: Implement**

Add to `crates/tessera_ui/src/view/panels.rs`:

```rust
/// The inspector's sections, in the order they are drawn.
///
/// The order is the whole design. Hiding a section moves everything below it,
/// so the sections that apply to every frame come first and the ones that can
/// be absent come last — and hiding one then never moves a control the user
/// reaches for often. A control that relocates by context is one the hand
/// cannot find without the eye.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Transform,
    Fill,
    Stroke,
    Text,
    Frame,
}

impl Section {
    /// Display order. Universal sections first; see the type's note.
    pub const ALL: [Section; 5] = [
        Section::Transform,
        Section::Fill,
        Section::Stroke,
        Section::Text,
        Section::Frame,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Transform => "Transform",
            Section::Fill => "Fill",
            Section::Stroke => "Stroke",
            Section::Text => "Text",
            Section::Frame => "Frame",
        }
    }

    /// Whether this section says anything about `frame`.
    pub fn applies_to(self, frame: &tessera_document::nodes::Frame) -> bool {
        use tessera_document::nodes::FrameKind;
        match self {
            // Every frame has a place, a fill and a stroke — even when the
            // stroke is None, which is a value the section can set.
            Section::Transform | Section::Fill | Section::Stroke => true,
            Section::Text => matches!(frame.kind, FrameKind::Text { .. }),
            Section::Frame => matches!(frame.kind, FrameKind::Group(_)),
        }
    }
}
```

Then restructure `inspector` so that, once it has a single selected frame, it loops `Section::ALL`, skips sections where `applies_to` is false, and draws each one's header and body. Move the existing position/size/rotation code into the `Transform` arm and the existing fill picker into the `Fill` arm; leave `Stroke`, `Text` and `Frame` bodies as the existing text handling plus stubs that later tasks fill.

- [ ] **Step 4: Amend the spec**

In `docs/superpowers/specs/2026-09-03-instrument-milestone-design.md`, replace D1's sentence about sections not moving with the ordering argument above. The claim as written was not implementable; the ordering is what makes it true.

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p tessera_ui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Give the inspector a fixed section order"
```

---

### Task 2: The reference point

**Files:**
- Modify: `crates/tessera_geometry/src/anchor.rs`, `crates/tessera_ui/src/app.rs`, `crates/tessera_ui/src/view/panels.rs`, `crates/tessera_ui/src/view/viewport.rs`
- Test: inline in `anchor.rs` and `panels.rs`

**Interfaces:**
- Consumes: `Anchor`, `Anchor::in_rect`, `Anchor::ALL` — all built in phase A.
- Produces:
  - `Anchor::shear(self, rect: DocRect, degrees: f64) -> Transform`
  - `TesseraApp::anchor: Anchor` — application state, not document data, and persistent across selections the way the active tool is
  - `panels::reference_proxy(ui, anchor: &mut Anchor) -> bool`

- [ ] **Step 1: Write the failing tests**

In `crates/tessera_geometry/src/anchor.rs`:

```rust
    #[test]
    fn a_shear_holds_its_anchor_still() {
        let r = rect();
        for anchor in Anchor::ALL {
            let fixed = anchor.in_rect(r);
            let moved = anchor.shear(r, 20.0).apply(fixed);
            assert!(close(fixed, moved), "{anchor:?} moved its own anchor point");
        }
    }

    #[test]
    fn a_shear_leans_the_top_of_a_box_sideways() {
        let r = rect();
        // About the bottom edge, so the bottom stays put and the top leans.
        let t = Anchor::BottomLeft.shear(r, 45.0);
        let top_left = DocPoint { x: r.x, y: r.y };
        let leaned = t.apply(top_left);
        assert!(
            (leaned.x - (top_left.x + r.height)).abs() < 1e-9,
            "45 degrees should lean the top by the box's own height, got {leaned:?}"
        );
        assert!((leaned.y - top_left.y).abs() < 1e-9, "and not move it vertically");
    }

    #[test]
    fn no_shear_is_the_identity() {
        assert!(Anchor::Centre.shear(rect(), 0.0).is_identity());
    }
```

In `crates/tessera_ui/src/view/panels.rs`:

```rust
    #[test]
    fn the_default_reference_point_is_the_centre() {
        // Scaling and rotating about the middle is what a user expects when
        // they have not said otherwise.
        assert_eq!(TesseraApp::headless().anchor, Anchor::Centre);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tessera_geometry anchor::` then `cargo test -p tessera_ui panels::`
Expected: FAIL — `no method named shear`, `no field anchor`.

- [ ] **Step 3: Implement the shear**

In `crates/tessera_geometry/src/anchor.rs`:

```rust
    /// Lean this rectangle's contents sideways about the anchor.
    ///
    /// A horizontal shear: points move along `x` in proportion to their
    /// distance from the anchor in `y`. Positive leans the top to the right.
    pub fn shear(self, rect: DocRect, degrees: f64) -> Transform {
        if degrees == 0.0 {
            return Transform::IDENTITY;
        }
        let about = self.in_rect(rect);
        let m = degrees.to_radians().tan();
        // translate(-about) then shear then translate(about), composed into
        // one matrix: x' = x + m*(y - about.y), y' = y.
        Transform::from_affine(kurbo::Affine::new([
            1.0,
            0.0,
            m,
            1.0,
            -m * about.y,
            0.0,
        ]))
    }
```

- [ ] **Step 4: Add the application state**

In `crates/tessera_ui/src/app.rs`, add to `TesseraApp`:

```rust
    /// The point transforms resolve about.
    ///
    /// Application state rather than document data, and persistent across
    /// selections the way the active tool is: it is a way of working, not a
    /// property of any one object.
    pub anchor: tessera_geometry::Anchor,
```

initialised in `headless` as `anchor: tessera_geometry::Anchor::default()`.

- [ ] **Step 5: Draw the proxy**

In `crates/tessera_ui/src/view/panels.rs`:

```rust
/// The nine-point reference proxy.
///
/// Bigger than InDesign's, which is a grid of targets a few pixels across —
/// small enough that hitting the wrong one is easy and noticing is not.
/// Returns whether the anchor changed.
pub fn reference_proxy(ui: &mut Ui, anchor: &mut Anchor) -> bool {
    const CELL: f32 = 14.0;
    let side = CELL * 3.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let mut changed = false;

    for (i, candidate) in Anchor::ALL.iter().enumerate() {
        let (col, row) = ((i % 3) as f32, (i / 3) as f32);
        let cell = egui::Rect::from_min_size(
            rect.min + Vec2::new(col * CELL, row * CELL),
            Vec2::splat(CELL),
        );
        let response = ui.interact(
            cell,
            ui.id().with(("anchor", i)),
            Sense::click(),
        );
        if response.clicked() {
            *anchor = *candidate;
            changed = true;
        }

        let selected = *candidate == *anchor;
        let dot = if selected { 4.0 } else { 2.0 };
        let colour = if selected {
            Theme::ACCENT
        } else {
            Theme::TEXT_MUTED
        };
        ui.painter().circle_filled(cell.center(), dot, colour);
    }

    ui.painter()
        .rect_stroke(rect, 2.0, egui::Stroke::new(1.0, Theme::BORDER), egui::StrokeKind::Inside);
    changed
}
```

Draw it at the head of the `Transform` section. If `rect_stroke`'s signature differs in egui 0.35, follow whatever the existing code in this file already calls — read it rather than guessing.

- [ ] **Step 6: Mark the anchor on the canvas**

In `crates/tessera_ui/src/view/viewport.rs`, where selection handles are drawn, also paint a small ring at `state.anchor.in_rect(bounds)` mapped through the frame's transform and the view.

This is D4, and it is the point of the whole task: InDesign's proxy silently changes what every field and every drag gesture mean, from a corner of the screen. Putting the mark on the object shows the mode where the user is already looking, which is the only place a mode can safely be displayed.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p tessera_geometry -p tessera_ui`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "Add the reference point, and draw it where it acts"
```

---

### Task 3: Transform fields that mean one thing

Position, size, scale, rotation and shear all become one path: read the frame's transform through `Decomposition`, write it back as a composition about the anchor.

**Files:**
- Modify: `crates/tessera_ui/src/view/panels.rs`, `crates/tessera_ui/src/command.rs`
- Test: inline in `command.rs`

**Interfaces:**
- Consumes: `Transform::decompose`, `Anchor::scale/rotate/shear`, `Unit`.
- Produces:
  - `Command::TransformAbout { id: FrameId, anchor: Anchor, scale: (f64, f64), rotate: f64, shear: f64 }` — each a **delta**, applied about `anchor`
  - `TesseraApp::constrain_proportions: bool`

Deltas rather than absolutes, because the anchor is what the operation is about: "make this 200% wide **about its centre**" is a different result from setting a width. Absolutes would have to reconstruct the translation themselves and would quietly ignore the reference point, which is the bug D4 exists to prevent.

- [ ] **Step 1: Write the failing tests**

In `crates/tessera_ui/src/command.rs`'s test module:

```rust
    #[test]
    fn scaling_about_the_centre_leaves_the_centre_where_it_was() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 100.0,
                y: 100.0,
                width: 40.0,
                height: 20.0,
            }),
        );
        let id = state.active().selection.single().expect("the new rectangle");
        let before = state.active().document().frame(id).expect("frame").centre();

        apply(
            &mut state,
            Command::TransformAbout {
                id,
                anchor: Anchor::Centre,
                scale: (2.0, 2.0),
                rotate: 0.0,
                shear: 0.0,
            },
        );

        let after = state.active().document().frame(id).expect("frame").centre();
        assert!((after.x - before.x).abs() < 1e-9);
        assert!((after.y - before.y).abs() < 1e-9);
    }

    #[test]
    fn scaling_about_a_corner_holds_that_corner_still() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 100.0,
                y: 100.0,
                width: 40.0,
                height: 20.0,
            }),
        );
        let id = state.active().selection.single().expect("the rectangle");
        let corner = state.active().document().frame(id).expect("frame").corners()[0];

        apply(
            &mut state,
            Command::TransformAbout {
                id,
                anchor: Anchor::TopLeft,
                scale: (3.0, 3.0),
                rotate: 0.0,
                shear: 0.0,
            },
        );

        let after = state.active().document().frame(id).expect("frame").corners()[0];
        assert!((after.x - corner.x).abs() < 1e-9, "the anchored corner moved");
        assert!((after.y - corner.y).abs() < 1e-9);
    }

    #[test]
    fn a_transform_is_one_undo_entry() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().expect("the rectangle");
        let before = state.active().document().frame(id).expect("frame").transform;

        apply(
            &mut state,
            Command::TransformAbout {
                id,
                anchor: Anchor::Centre,
                scale: (1.0, 1.0),
                rotate: 30.0,
                shear: 10.0,
            },
        );
        apply(&mut state, Command::Undo);

        let after = state.active().document().frame(id).expect("frame").transform;
        assert_eq!(after, before, "one undo unwinds the whole edit");
    }

    #[test]
    fn shear_survives_a_round_trip_through_the_decomposition() {
        // The reason phase A's decompose exists: rotation_degrees assumed no
        // shear, and this is the first code that breaks that assumption.
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 50.0,
            }),
        );
        let id = state.active().selection.single().expect("the rectangle");
        apply(
            &mut state,
            Command::TransformAbout {
                id,
                anchor: Anchor::Centre,
                scale: (1.0, 1.0),
                rotate: 0.0,
                shear: 15.0,
            },
        );

        let d = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .transform
            .decompose();
        assert!(
            (d.shear_degrees - 15.0).abs() < 1e-6,
            "read back {} degrees of shear",
            d.shear_degrees
        );
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tessera_ui command::`
Expected: FAIL, `no variant named TransformAbout`.

- [ ] **Step 3: Implement the command**

Add the variant to `Command`, ensure `mutates()` is true for it, and handle it:

```rust
        Command::TransformAbout {
            id,
            anchor,
            scale,
            rotate,
            shear,
        } => {
            let Some(frame) = state.active().document().frame(id) else {
                return;
            };
            // The anchor is resolved in document space, against where the
            // frame really is — not against `bounds`, which says only where
            // it is in its own space and does not move when the frame does.
            let placed = frame.bounds;
            let existing = frame.transform;

            let about = |t: Transform| existing.then(t);
            let mut result = existing;
            if scale != (1.0, 1.0) {
                result = about(anchor.scale(placed, scale.0, scale.1));
            }
            if rotate != 0.0 {
                result = result.then(anchor.rotate(placed, rotate));
            }
            if shear != 0.0 {
                result = result.then(anchor.shear(placed, shear));
            }

            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.transform = result;
            }
        }
```

Compose in one command so the whole edit is one undo entry. If the anchor must be resolved in document space rather than frame space, adjust `placed` accordingly and add a test pinning which space it is — do not leave it ambiguous.

- [ ] **Step 4: Build the fields**

In the `Transform` section of the inspector, after the reference proxy:

- **X, Y, W, H** through `measure`, in the preferred unit — reuse the helper phase B added rather than writing a second one.
- **A constrain-proportions chain** between W and H, toggling `state.constrain_proportions`; when on, editing one scales the other by the same ratio.
- **Scale X %, Scale Y %**, read from `transform.decompose()` and written as a delta ratio.
- **Rotation** and **Shear** in degrees, read from the decomposition and written as deltas.

Every field emits **one** `Command` on a completed edit, never per frame of a drag.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p tessera_ui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Make every transform field resolve about the reference point"
```

---

### Task 4: The stroke section

The largest single piece of unreachable capability in the codebase.

**Files:**
- Modify: `crates/tessera_ui/src/view/panels.rs`, `crates/tessera_ui/src/command.rs`
- Test: inline in `command.rs`

**Interfaces:**
- Consumes: `Stroke`, `StrokeAlign`, `LineCap`, `LineJoin` — all shipped since milestone 0 and exposed nowhere.
- Produces: `Command::SetStroke { id: FrameId, stroke: Option<Stroke> }`

`Option`, because "no stroke" is a value the section must be able to set and is the state every shape starts in.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn a_stroke_can_be_given_and_taken_away() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().expect("the rectangle");
        assert!(state.active().document().frame(id).expect("frame").stroke.is_none());

        let stroke = Stroke {
            color: Color::BLACK,
            width: 3.0,
            align: StrokeAlign::Inside,
            cap: LineCap::Round,
            join: LineJoin::Bevel,
            miter_limit: 4.0,
            dashes: vec![6.0, 3.0],
            dash_offset: 0.0,
        };
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(stroke.clone()),
            },
        );
        assert_eq!(
            state.active().document().frame(id).expect("frame").stroke,
            Some(stroke)
        );

        apply(&mut state, Command::SetStroke { id, stroke: None });
        assert!(state.active().document().frame(id).expect("frame").stroke.is_none());
    }

    #[test]
    fn setting_a_stroke_is_one_undo_entry_covering_every_property() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().expect("the rectangle");

        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(Stroke::new(Color::BLACK, 2.0)),
            },
        );
        apply(&mut state, Command::Undo);
        assert!(
            state.active().document().frame(id).expect("frame").stroke.is_none(),
            "one undo removes the whole stroke, not one of its fields"
        );
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p tessera_ui command::`
Expected: FAIL, `no variant named SetStroke`.

- [ ] **Step 3: Implement the command**

```rust
        Command::SetStroke { id, stroke } => {
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.stroke = stroke;
            }
        }
```

- [ ] **Step 4: Build the section**

The `Stroke` arm of the inspector shows, in this order:

- a checkbox turning the stroke on and off — off writes `None`;
- **Weight**, through `measure` in the preferred unit;
- **Colour**, through the existing `fill_picker` helper;
- **Align** — Centre, Inside, Outside, as a segmented row of three;
- **Cap** — Butt, Round, Square;
- **Join** — Miter, Round, Bevel, with **Miter limit** shown only when Miter is chosen, because it means nothing otherwise;
- **Dashes** — a preset row (Solid, Dashed, Dotted) writing the corresponding `dashes` vector, plus a **Dash offset** field shown only when the stroke is dashed.

Turning the stroke on from `None` uses `Stroke::new(Color::BLACK, 1.0)`, which the model already defines as what everything drew before the extra properties existed.

Each completed edit emits one `Command::SetStroke` carrying the whole struct.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p tessera_ui`
Expected: PASS.

- [ ] **Step 6: Look at it**

Run: `cargo run --release -p tessera_app`

Draw a rectangle. Give it a 3 pt dashed stroke aligned inside with round caps. It should appear on the canvas — the renderer has honoured all of this since milestone 0 and has never been asked.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Make the stroke model reachable"
```

---

### Task 5: The fill and stroke proxy

**Files:**
- Modify: `crates/tessera_ui/src/view/panels.rs`, `crates/tessera_ui/src/view/mod.rs`, `crates/tessera_ui/src/command.rs`
- Test: inline in `command.rs`

**Interfaces:**
- Produces:
  - `Command::SwapFillAndStroke(FrameId)`
  - `Command::DefaultFillAndStroke(FrameId)` — black fill, no stroke
  - `Command::ClearFill(FrameId)`

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn swapping_exchanges_the_fill_and_the_stroke_colour() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().expect("the rectangle");
        apply(
            &mut state,
            Command::SetFill {
                id,
                color: Color::WHITE,
            },
        );
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(Stroke::new(Color::BLACK, 2.0)),
            },
        );

        apply(&mut state, Command::SwapFillAndStroke(id));

        let frame = state.active().document().frame(id).expect("frame").clone();
        assert_eq!(frame.fill, Color::BLACK);
        assert_eq!(
            frame.stroke.expect("still stroked").color,
            Color::WHITE,
            "the stroke keeps its width and only its colour swaps"
        );
    }

    #[test]
    fn swapping_a_shape_with_no_stroke_gives_it_one() {
        // Otherwise the swap silently discards the fill.
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().expect("the rectangle");
        apply(&mut state, Command::SwapFillAndStroke(id));

        let frame = state.active().document().frame(id).expect("frame").clone();
        assert!(frame.stroke.is_some(), "the fill became a stroke");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p tessera_ui command::`
Expected: FAIL, `no variant named SwapFillAndStroke`.

- [ ] **Step 3: Implement**

Handle the three variants. Swapping a shape with no stroke gives it a stroke of the fill's colour at 1 pt and leaves the fill black, rather than discarding the fill.

- [ ] **Step 4: Draw the proxy and bind the keys**

At the head of the inspector, above the sections: two overlapping swatches — fill behind, stroke in front — with a swap arrow and a small default marker, the arrangement every drawing tool since MacDraw has used and the one place InDesign's design is worth copying exactly.

In `crates/tessera_ui/src/view/mod.rs`'s `accelerators`, bind `X` to swap, `D` to defaults and `/` to none. Guard every one of them so they do nothing while a text frame is being edited — otherwise typing the letter `x` into a caption swaps the frame's colours, which is the kind of bug that makes a tool feel hostile.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p tessera_ui`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Add the fill and stroke proxy, with its three keys"
```

---

## Closing the plan

- [ ] **Run the full non-GPU suite**

```bash
cargo test -p tessera_geometry -p tessera_color -p tessera_io -p tessera_text -p tessera_document -p tessera_layout -p tessera_pdf -p tessera_ui
```

- [ ] **Run clippy**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Perform this plan's sentence, by hand**

> Select a rectangle. Set the reference point to its centre and watch the mark appear on the object. Type `12mm` into a width field and see it convert. Scale it to 200% about that centre and watch the centre stay put; move the reference point to a corner and scale again, and watch that corner stay put instead. Shear it 15 degrees. Give it a 3 pt dashed stroke, aligned inside, with round caps, and see it on the canvas. Press `X` and watch the fill and stroke exchange. Undo each of those once, and have each one unwind whole.

- [ ] **Tick C1 to C5 in the roadmap**, recording anything left partial.

- [ ] **Write the C-ii plan** — align and distribute, the canvas toolbar, rulers and guides, screen modes.
