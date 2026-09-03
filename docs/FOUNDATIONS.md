# Foundations

What the base needs before more features sit on top of it.

Ordered by **how much more expensive each one gets the longer it waits** —
not by how visible it is. Everything in Tier 1 touches either the file format
or every gesture in the application, so each feature added before them is a
feature that has to be revisited after them.

First written against the code as it stood on 2026-09-02, with the evidence
for each claim. It is not a plan; it is the list a plan should be made from.

**Updated 2026-09-03.** Tier 1.1 and 1.3 are done and Tier 1.2 is half done;
each section below says what landed. The entries are kept rather than deleted,
because what a thing was is most of why it is now shaped the way it is.

---

## What is already sound

Worth stating, so the list below is not read as a verdict on the whole thing.

- **Crate layering.** Ten crates, dependencies pointing downward only,
  `tessera_pdf` unable to reach `tessera_render`. That separation is what
  makes an export provably independent of the GPU.
- **One shaping source.** `PositionedGlyph` is the shared currency of the
  renderer and the PDF writer, so an export cannot drift from the screen.
  The caret work in `tessera_text::caret` respected this rather than widening
  it.
- **Colour is already print-shaped.** `Color` is `Rgb | Cmyk | Spot`, with
  the screen conversion documented as an approximation pending ICC. Most
  projects get this wrong by starting RGB-only; retrofitting CMYK afterwards
  is brutal. This one is done.
- **The format has a migration chain**, exercised by tests. That is what
  makes the Tier 1 items *possible* rather than terrifying: the document
  model can change.
- **Undo is one entry per gesture**, not per mutation.
- **CI builds and runs headless tests on Linux, macOS and Windows** on every
  push.

---

## Tier 1 — before any more features

### 1. A frame's transform is a rectangle and an angle, not a matrix — DONE

**Landed 2026-09-03, format version 3.** `Frame` now holds `bounds` in its own
coordinate space plus a `Transform` placing that space on the page. `resize`
lost its rotation argument entirely — the pointer arrives in the frame's own
space, so a turned, sheared or flipped frame resizes with the same arithmetic
as an upright one. `scale_origins` and `rotate_origins` collapsed into one
`footprint_map`, which the inspector's fields now route through too. Scaling a
rotated group is exact rather than approximate; a test asserts a 45-degree
child comes out with non-perpendicular axes, which is the shear the old model
had to throw away.

The problem it solved, kept for the record:

`Frame { bounds: DocRect, rotation: f64 }` in
`crates/tessera_document/src/nodes.rs`.

An axis-aligned box plus one angle cannot express shear, and cannot express
what happens when a rotated object is scaled non-uniformly. This is not
hypothetical — the group transform work already ran into it, and the
approximation is admitted in a comment in
`crates/tessera_ui/src/transform.rs`:

> A child turned at some angle of its own still has its width and height
> scaled along its own axes. An axis-aligned box plus an angle cannot express
> the shear that would otherwise be needed, and refusing to scale at all would
> be worse than approximating.

InDesign gives every object a full affine. Until Tessera does:

- shear/skew cannot be built at all;
- scaling a rotated group silently distorts its children;
- nested group transforms do not compose correctly;
- "flip horizontal/vertical" has nowhere to live.

**Cost of doing it now:** `nodes.rs` (and therefore the format, plus a
migration), `transform.rs`, the gestures in `viewport.rs`, `resolve.rs`,
`scene.rs`, the PDF writer, and hit testing. Roughly the surface the last
rework already touched.

**Cost of doing it later:** the same, plus every feature added in between —
images, styles, tables, anchored objects — each of which will have grown its
own assumption that an object is a box and an angle.

### 2. Undo stores whole documents — partly addressed, and deliberately not finished

`crates/tessera_document/src/history.rs`: a full clone of the document per
gesture, up to 200 resident. Correct, simple, and right for a walking skeleton;
it does not survive a 200-page catalogue.

**Done: the stack is now bounded by memory as well as by count**
(`DEFAULT_BUDGET_BYTES`, with `Document::footprint` as the estimate). A count
alone cannot tell two hundred snapshots of a flyer from two hundred of a
catalogue. One entry is always kept, however large.

**Not done: delta undo.** Two things came out of looking at it properly, and
both argue for deciding this deliberately rather than in passing.

**`SlotMap` has no insert-at-key.** Its `insert` mints a fresh generational
key, so a deleted frame *cannot* be restored under its original `FrameId`. Any
delta scheme therefore has to fall back to a whole snapshot whenever the key
set shrinks — and every `FrameId` held elsewhere (the selection, a group's
child list) would be stale across an undo. That is not a detail; it decides
whether deltas are worth having at all.

**The existing design chose snapshots on purpose, having been bitten.** The
module's own note records it: snapshots "cannot develop the class of bug where
an inverse operation is subtly wrong — or, as happened in the previous
codebase, where an operation never got an inverse at all and was silently not
undoable." Reversing a decision made *because of a bug that already cost this
project once* should be a conscious call.

Three ways forward, in rough order of how much they change:

1. **Persistent arenas.** Replace `SlotMap` with a structurally-shared map, so
   `Document::clone` is O(changed) and the snapshot API and semantics do not
   change at all. Lowest risk to correctness, because no inverse is ever
   written; highest cost in dependency and model churn, and it needs a key
   scheme that survives it.
2. **Automatic diffing.** Keep one baseline document, and after each command
   diff it against the current one to store only the frames that changed.
   Needs `record` to move from before-the-mutation to after it, which moves the
   boundary of a text-editing session — that boundary is currently opened by
   hand in `start_editing`. Snapshot whenever the key set changes.
3. **Per-command inverses.** Smallest storage, and exactly the design that
   failed before. Would need a property test over random command sequences
   asserting apply-then-undo returns an equal document, as a standing guard.

My recommendation is (1), and it is not a change to make unattended.

### 3. The entire document is re-resolved and re-shaped every frame — DONE

**Landed 2026-09-03.** Two caches, at the two levels where the question
differs. `tessera_layout::ResolveCache` keys the whole resolved document on the
revision counter, so a still canvas lays out nothing at all. The `Shaper` keys
laid-out stories on text, family, size, line height and measure — exhaustive by
construction, since the brush type is `()` and colour cannot move a glyph —
which is what makes *dragging* cheap, because a drag bumps the revision on
every pointer move but changes neither a story's text nor its measure.

Still outstanding here: **viewport culling**. Panning and zooming rebuild the
Vello scene because the camera is baked into it, and off-screen spreads are
still built. That needs page structure Milestone 3 has not built yet.

The problem it solved, kept for the record:

`crates/tessera_ui/src/view/viewport.rs`, in `show`:

```rust
let resolved = tessera_layout::resolve::resolve(&state.document, &mut state.shaper);
let scene = tessera_render::scene::build_scene(&resolved, scaled_view(state, ppp), ...);
```

Unconditional, every frame, at display refresh rate. `resolve` re-runs parley
layout for **every text frame in the document** whether or not anything
changed, whether or not it is on screen.

This is the item most likely to be mistaken for "Rust is slow" or "Vello is
slow" later. It is neither; it is a missing cache.

The hook already exists: `Document` carries a `revision` counter that
`frame_mut` bumps. What is needed is a resolved-document cache keyed on it,
per-frame invalidation rather than whole-document, and viewport culling so
off-screen spreads cost nothing.

**Do this before Milestone 2 (Typography).** Every style, run and threading
feature multiplies the per-frame shaping cost this currently pays in full.

---

## Tier 2 — structural, but additive

These change the document model without invalidating what is already there,
so they are ordinary feature work that happens to touch the format.

- **Image frames.** `FrameKind` has no `Image` variant. Pulls in linked vs
  embedded assets, missing-link and relink handling, effective resolution
  reporting, and clipping paths. Table stakes for a layout tool.
- ~~**A real stroke model.**~~ **Done 2026-09-03, format version 4.** Stroke
  carries alignment, cap, join, miter limit, dashes and a dash offset.
  Alignment was the structural one — it changes geometry, so `Stroke::offset`
  lives in the model and both renderers move the geometry by it before
  stroking centred. Nothing needed rewriting: every new field defaults to what
  a stroke drew before it existed.

  It uncovered an export bug on the way: the PDF writer filled rectangles and
  ellipses and never stroked them, so a framed box exported without its frame.
  Fixed.

  Still open here: **a path's alignment is not applied.** Offsetting an
  arbitrary curve is a different problem from insetting a rectangle, and needs
  either a real offsetting pass or a clip layer. Paths stroke centred.
- **Text runs and styles.** Already Milestone 2. `Story` currently applies one
  `TextStyle` to the whole text, which the code notes is a deliberate M0
  simplification.

## Tier 3 — verification, not architecture

**"Runs on Linux and macOS" is currently unproven.** CI compiles the
workspace and runs headless tests on all three, which is real but narrow. It
does not exercise:

- Vello/wgpu against Mesa/Vulkan on Linux, or Metal on macOS — the GPU tests
  are `#[ignore]`d and have only ever run on this Windows machine;
- system font enumeration through fontique, which is per-platform;
- HiDPI and fractional scaling, where the canvas pixel-snapping fix matters;
- file dialogs, and macOS menu/shortcut conventions (Cmd rather than Ctrl).

This needs someone to run the application on each platform, not more CI. It
is listed last because it is a risk to *discover*, not a design to *change* —
but it should happen before the base is called done, because a rendering or
font surprise on one platform can be an architectural problem wearing a bug's
clothing.

---

## Suggested order

1. ~~Affine transforms (Tier 1.1)~~ — **done**, format version 3.
2. ~~Incremental resolve and shaping (Tier 1.3)~~ — **done**, two caches.
3. Delta undo (Tier 1.2) — **memory bound done; the design decision is open**
   and wants a person. See the three options above.
4. Run it on Linux and macOS (Tier 3) — cheap, and may reorder everything
   below.
5. ~~The stroke model~~ — **done**. Then images, then Milestone 2 as written.

## What is left, shortest first

- **Run it on Linux and macOS.** Still the largest unknown, and cheap.
- **Viewport culling.** Panning still rebuilds the whole scene; off-screen
  spreads are still built. Needs Milestone 3's page structure.
- **Stroke alignment on paths.** Needs offsetting or a clip layer.
- **Delta undo.** The design decision is written up above and wants a person.
- **Image frames.** The one Tier 2 item untouched, and the biggest.
