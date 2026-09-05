# Milestone 2 Phase 3 — Shaping Runs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a bold word appear — on the canvas and in the exported PDF — by shaping a story's runs instead of its one style.

**Architecture:** `ShapedText` currently carries a single `font_size` for a whole story, and both consumers group glyphs by font and use it once. Runs mean a size per span, so the shaped form gains a `ShapedRun` layer that mirrors what parley already hands over, and both consumers iterate runs instead of filtering glyphs by font index.

**Spec:** [milestone 2 typography](../specs/2026-09-05-milestone-2-typography-design.md) — D1, D2.

**Preceded by:** phase 1 (the run model) and phase 2 (format version 6).

## Why this is three crates at once

`PositionedGlyph` is decision D3's shared currency: `tessera_render` and `tessera_pdf` consume exactly the same type, and that is what guarantees an export matches the screen. A size that varies per run cannot be added to one side only.

The blast radius is smaller than it looks. `tessera_layout` touches `shaped.lines.len()` and nothing else; `caret.rs` only names the type in a doc comment. The real work is in `shape.rs`, `scene.rs` and `writer.rs`.

## Global Constraints

- `unsafe_code = "forbid"`. Document units are **points**.
- **No format version bump in this phase.** `Story::style` stays as the
  story's default formatting; retiring it belongs with the style tables in
  phase 4, so that one bump carries both rather than two carrying one each.
- `run.style` — the named-style reference — is **not resolved here.** Nothing
  can set one yet, and the tables live on the document where `tessera_text`
  cannot see them. Phase 3 resolves `run.local` over the story's default,
  which is all "a bold word" needs.
- Tests land in the same commit. `cargo clippy --workspace --all-targets -- -D warnings` clean.
- GPU tests run alone, in the foreground, at the end.

---

### Task 1: A shaped run

**Files:** modify `crates/tessera_text/src/shape.rs`

**Produces:**
- `ShapedRun { font_index: usize, size: f32, glyphs: Vec<PositionedGlyph> }`
- `ShapedLine { runs: Vec<ShapedRun>, baseline: f64 }` — `glyphs` becomes a
  method that flattens, so the few callers that want all of them still can
- `ShapedText` **loses** `font_size`

Mirroring parley's own `GlyphRun` rather than putting a size on every glyph: the shaper already loops over glyph runs to build this, so it is less work, not more, and it stops the size being repeated once per glyph.

- [ ] **Step 1: Write the failing tests** — a run carries the size it was shaped at; `ShapedLine::glyphs()` returns every glyph in run order; an empty story shapes to no runs.
- [ ] **Step 2: Run to verify they fail.** `cargo test -p tessera_text shape::`
- [ ] **Step 3: Implement**, building a `ShapedRun` per parley `GlyphRun` and taking the size from `run.run().font_size()`.
- [ ] **Step 4: Run, then commit** — `Give a shaped line runs, each with its own size`

---

### Task 2: Push the story's runs into the layout

**Files:** modify `crates/tessera_text/src/shape.rs`

**Produces:**
- `Story::resolved_format(&self, run: &Run) -> CharacterFormat` — `run.local`
  over the story's default
- `Shaper::layout` pushes a parley span per run

parley's `RangedBuilder::push(property, range)` is exactly this mechanism, so each run's family, size, weight, italic and line height become spans over its own range.

**The cache key has to change too.** `ShapeKey` is built from the story's single style; two stories differing only in their runs would collide and the second would be handed the first's layout. That is a wrong answer, not a slow one.

- [ ] **Step 1: Write the failing tests** — a story with a 24pt run and a 12pt run shapes to runs of both sizes; a bold run reports a different font from a regular one where the family has both faces, or, if it does not, at least shapes without error; **two stories with identical text and different runs do not collide in the cache**.
- [ ] **Step 2–4:** implement, run, commit — `Shape a story run by run`

---

### Task 3: Draw the runs

**Files:** modify `crates/tessera_render/src/scene.rs`

`draw_text` groups by font index and calls `.font_size(shaped.font_size)` once. It becomes a loop over runs, one `draw_glyphs` per run.

- [ ] **Step 1: Write the failing test** — a two-run shaped text produces more encoding than a one-run one, so both runs really reached the scene.
- [ ] **Step 2–4:** implement, run, commit — `Draw each run at its own size`

---

### Task 4: Export the runs

**Files:** modify `crates/tessera_pdf/src/writer.rs`

Two places use `shaped.font_size`: the advance normalisation in `collect_fonts`, and `set_font` in the content stream. Both become per-run.

**The advance normalisation is the one to get right.** A glyph's advance is carried in points at the size it was shaped at, so dividing by the wrong size silently produces a PDF whose text is correctly positioned but whose widths are wrong — which a viewer will not complain about and a printer will.

- [ ] **Step 1: Write the failing tests** — a document with two sizes exports; the content stream sets the font more than once; a glyph's recorded width matches its advance at **its own** run's size.
- [ ] **Step 2–4:** implement, run, commit — `Export each run at its own size`

---

### Task 5: Prove it end to end

- [ ] **Step 1:** extend `crates/tessera_render/tests/gpu_render.rs` — a story whose second run is twice the size of the first puts dark pixels lower down the page than a uniform one would, so the size really reached the rasteriser.
- [ ] **Step 2:** run the GPU suite alone, in the foreground.
- [ ] **Step 3:** extend the A7 performance guard to a text-heavy document. It currently measures 500 rectangles and would not notice shaping getting slower, which is precisely what this phase risks.
- [ ] **Step 4:** commit — `Prove a run's size reaches the pixels and the PDF`

---

## Closing the phase

- [ ] Full non-GPU suite; GPU suite alone; clippy at `-D warnings`.
- [ ] **Perform the sentence, by hand:**

> Draw a text frame and type into it. Nothing has changed. Then — with no interface for it yet — open a document whose story has two runs at different sizes and see both, on the canvas and in an exported PDF opened in Acrobat.

  The second half needs a fixture rather than a gesture, because the inspector that would set a run's size is phase 4. Say so in the roadmap rather than implying a person can do it from the interface.

- [ ] **Write the phase 4 plan:** the style tables, the full four-level cascade, format version 7 retiring `Story::style`, and the typography inspector.
