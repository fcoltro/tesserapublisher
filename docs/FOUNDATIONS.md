# Foundations

What the base needs before more features sit on top of it.

Ordered by **how much more expensive each one gets the longer it waits** —
not by how visible it is. Everything in Tier 1 touches either the file format
or every gesture in the application, so each feature added before them is a
feature that has to be revisited after them.

This is an assessment of the code as it stands on 2026-09-02, with the
evidence for each claim. It is not a plan; it is the list a plan should be
made from.

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

### 1. A frame's transform is a rectangle and an angle, not a matrix

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

### 2. Undo stores whole documents

`crates/tessera_document/src/history.rs`: `past: VecDeque<Document>`, and
`record` is `self.past.push_back(doc.clone())`, with a limit of 200.

A full clone of the document per gesture, and up to 200 of them resident.
Correct, simple, and completely right for a walking skeleton. It does not
survive a real document: a 200-page catalogue with thousands of frames and
long stories makes every edit an O(document) copy and the undo stack a
multi-gigabyte structure.

The replacement is delta or command-based undo — store the inverse of what
changed, not the whole world. Touches `command.rs` and every mutation path,
which is small **now** and grows with every command added.

### 3. The entire document is re-resolved and re-shaped every frame

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
- **A real stroke model.** `Stroke { color, width }` has no cap, join, dash,
  or alignment. Alignment (inside/centre/outside) is the structural one: it
  changes the geometry that gets rendered and exported, so it cannot be
  bolted on at the paint site.
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

1. Affine transforms (Tier 1.1) — one format migration, done while the
   surface is still small.
2. Incremental resolve and shaping (Tier 1.3) — before Milestone 2 makes text
   more expensive.
3. Delta undo (Tier 1.2) — before more commands exist to convert.
4. Run it on Linux and macOS (Tier 3) — cheap, and may reorder everything
   above.
5. Then images and the stroke model, then Milestone 2 as written.
