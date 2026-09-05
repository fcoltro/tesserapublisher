# Finishing Milestone 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Close every open item in milestone 2, so the acceptance sentence can
be performed in full rather than in part.

**Architecture:** One change is architectural and the rest are contained. The
shaper currently builds **one parley layout per story**, which is why indents,
paragraph spacing, hyphenation, drop caps and mixed alignment cannot be
expressed — parley aligns and measures a whole layout at once. Task 3 replaces
that with **one layout per paragraph, stacked**, and everything else in the
milestone either precedes it or falls out of it.

**Tech Stack:** parley 0.11, fontique 0.11, `tessera_text`, `tessera_render`,
`tessera_pdf`, `tessera_ui`.

**Spec:** `docs/superpowers/specs/2026-09-05-milestone-2-typography-design.md`

## Global Constraints

- `unsafe_code = "forbid"` workspace-wide.
- Every document mutation goes through `Command`; a live gesture may bypass it
  only with an adjacent `undo-bracketed:` comment.
- **`cargo fmt --all --check`, `cargo test --workspace`, and
  `cargo clippy --workspace --all-targets -- -D warnings` all clean before any
  commit.** CI checks all three; formatting was missed once already.
- British spelling in identifiers and copy.
- A control that sets a value nothing draws is worse than one that is absent.
- A migration test is built from a hand-made archive and checked by disabling
  the migration to confirm it goes red.

## What is open, and why

| Item | Blocked by |
|---|---|
| Font enumeration on macOS and Linux | nothing — needs a test, not code |
| Small caps | nothing — a font feature |
| Baseline shift | nothing — a glyph offset |
| Indents, space before/after, mixed alignment | one layout per paragraph |
| Hyphenation | one layout per paragraph, plus a dictionary |
| Drop caps | one layout per paragraph |
| Upper/lower case | offset mapping between shaped and stored text |
| Kerning | not modelled at all |
| H&J parameters | not modelled; parley has no justification limits |
| Right-to-left and bidi | untested; caret assumes logical order |

---

### Task 1: Font enumeration, everywhere

**Files:** `crates/tessera_text/src/shape.rs`

The roadmap marks this partial because it was only ever run on Windows. CI now
runs ubuntu, windows and macos, so the claim can be earned rather than
asserted.

- [ ] **Step 1:** Add a test that names the platform in its failure message, so
  a CI failure says which runner disagreed.
- [ ] **Step 2:** Run locally; push and read the three CI jobs.
- [ ] **Step 3:** Tick the roadmap item, or record what the other platforms do
  differently. **A Linux CI runner may genuinely have no fonts** — if so the
  test must say that is the finding rather than be weakened until it passes.
- [ ] **Step 4: Commit** — `Earn the claim that fonts enumerate everywhere`

---

### Task 2: Small caps and baseline shift

**Files:** `crates/tessera_text/src/shape.rs`, `crates/tessera_ui/src/view/panels.rs`

Both are modelled and neither reaches parley. Small caps is the `smcp` font
feature; baseline shift moves a run's glyphs off the baseline and belongs after
shaping, since it changes no advance and must not disturb line breaking.

`Case::Upper` and `Case::Lower` are **not** in this task — they change the text
being shaped, which moves every byte offset the caret depends on. That is task
6.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn small_caps_asks_the_font_for_them() {
    // A feature, not a transformation: the text is unchanged, so every byte
    // offset the caret uses is unchanged too.
    let mut story = Story::new("abc");
    story.runs[0].local.case = Some(Case::SmallCaps);
    let mut shaper = Shaper::new();
    let with = shaper.shape(&story, &NoStyles::default(), 400.0);
    let without = shaper.shape(&Story::new("abc"), &NoStyles::default(), 400.0);
    assert_ne!(
        with.runs().flat_map(|r| r.glyphs.iter().map(|g| g.glyph_id)).collect::<Vec<_>>(),
        without.runs().flat_map(|r| r.glyphs.iter().map(|g| g.glyph_id)).collect::<Vec<_>>(),
        "small caps must select different glyphs"
    );
}

#[test]
fn a_baseline_shift_moves_the_glyphs_and_not_the_line() {
    let mut story = Story::new("abc");
    story.runs[0].local.baseline_shift = Some(6.0);
    let mut shaper = Shaper::new();
    let raised = shaper.shape(&story, &NoStyles::default(), 400.0);
    let flat = shaper.shape(&Story::new("abc"), &NoStyles::default(), 400.0);

    let y = |t: &ShapedText| t.runs().next().and_then(|r| r.glyphs.first()).map(|g| g.y);
    assert!(y(&raised).zip(y(&flat)).is_some_and(|(a, b)| a < b), "raised");
    assert_eq!(raised.height, flat.height, "and the line did not grow");
}
```

- [ ] **Step 2:** Run to verify they fail.
- [ ] **Step 3:** Implement. `StyleProperty::FontFeatures(FontFeatures::from("smcp"))`
      pushed per run for `Case::SmallCaps`; baseline shift subtracted from each
      glyph's `y` when the run is built.
- [ ] **Step 4:** Run tests.
- [ ] **Step 5:** Give both controls a home in the inspector, out from under the
      "not yet drawn" heading.
- [ ] **Step 6: Commit** — `Draw small caps and baseline shift`

---

### Task 3: One layout per paragraph

**Files:** `crates/tessera_text/src/shape.rs`, `crates/tessera_text/src/caret.rs`,
`crates/tessera_render/src/scene.rs`, `crates/tessera_pdf/src/writer.rs`,
`crates/tessera_ui/src/view/panels.rs`

The architectural change. Today `Shaper::layout` builds one parley layout for a
whole story. Instead: split the text at paragraph boundaries, build one layout
per paragraph with its own measure and alignment, and stack them with the space
each asks for.

**Interfaces:**
- Produces `ShapedText { paragraphs: Vec<ShapedParagraph>, height, fonts }`
  where a `ShapedParagraph` carries its lines, its origin, and the byte range it
  covers.
- `Shaper::layout` becomes private to the paragraph loop; the caret's three
  entry points take an offset, find the paragraph holding it, and work inside
  that paragraph's layout with its origin added back.

**The risk worth naming:** the caret is the part most likely to break silently.
`offset_at` maps a point to a byte; if the paragraph lookup is wrong the caret
lands in the wrong paragraph and every subsequent edit corrupts the wrong text.
The existing property test over generated edit sequences is what guards this,
plus a new round-trip test: for every offset in a multi-paragraph story,
`offset_at(caret_geometry(offset))` returns the offset it started from.

- [ ] **Step 1: Write the failing tests** — a two-paragraph story where the
      paragraphs have different alignments lays each out its own way; an indent
      narrows only its own paragraph; space-before moves only what follows it;
      the caret round-trip above.
- [ ] **Step 2:** Run to verify they fail.
- [ ] **Step 3:** Implement the paragraph loop in `shape.rs`.
- [ ] **Step 4:** Move the caret onto per-paragraph layouts.
- [ ] **Step 5:** Update the renderer and the PDF writer to walk paragraphs.
- [ ] **Step 6:** Run the whole suite, including the GPU tests.
- [ ] **Step 7:** Restore the five withdrawn controls in the inspector.
- [ ] **Step 8: Commit** — `Lay out each paragraph on its own`

---

### Task 4: Hyphenation

**Files:** `crates/tessera_text/src/shape.rs`, `Cargo.toml`

parley does not hyphenate. A dictionary is needed; `hypher` is the small
Liang-algorithm crate with embedded patterns.

**Decide before implementing:** `hypher` embeds patterns per language and adds
weight. If it costs more than a few hundred kilobytes for English, record that
and keep hyphenation behind the paragraph flag rather than on by default.

- [ ] **Step 1:** Measure the dependency's real cost, and record it.
- [ ] **Step 2–5:** Test, implement, run, commit — `Hyphenate a justified paragraph`

---

### Task 5: Drop caps

**Files:** `crates/tessera_text/src/shape.rs`, `crates/tessera_text/src/story.rs`

A drop cap is the first N characters set at the height of M lines, with the
following lines indented past it. Needs `drop_cap_lines` and
`drop_cap_characters` on `ParagraphFormat` — the one part of this milestone that
adds model, and so the one part that needs a format version bump.

- [ ] **Step 1–6:** Model, migration with a hand-built archive, shape, test,
      control, commit — `Set a drop cap`

---

### Task 6: Upper, lower and the offset map

**Files:** `crates/tessera_text/src/shape.rs`

Shaping uppercase text means shaping a *different string*: `ß` becomes `SS`, so
byte offsets move. The layout must therefore carry a map from shaped offsets
back to stored offsets, and every caret entry point must go through it.

- [ ] **Step 1:** Test that a caret in text set to All Caps still selects the
      stored characters, including one whose uppercase form is longer.
- [ ] **Step 2–5:** Implement, run, control, commit — `Set text in capitals`

---

### Task 7: Kerning and H&J

**Files:** `crates/tessera_text/src/story.rs`, `crates/tessera_text/src/shape.rs`

Kerning is not modelled at all — the acceptance sentence asks for it and the
roadmap's checklist never listed it. Manual kerning is an adjustment *between*
two characters, so it is keyed on a position rather than on a range.

H&J needs word- and letter-spacing limits, which parley's justification does not
expose. **This task may end in a recorded refusal** rather than an
implementation: if parley cannot express it, say so in the roadmap with what
would be needed, rather than shipping controls that do nothing.

- [ ] **Step 1:** Establish what parley can and cannot do here, and record it.
- [ ] **Step 2–5:** Implement what it can; record what it cannot.

---

### Task 8: Right-to-left and bidirectional text

**Files:** `crates/tessera_text/src/caret.rs`, tests

parley shapes bidi already. The caret is what has never been tested against it,
and its arithmetic assumes visual order follows logical order.

- [ ] **Step 1:** Write tests with Arabic and Hebrew text and a mixed run, over
      the caret round-trip from task 3. **Expect failures**; they are the point.
- [ ] **Step 2:** Fix what they find, or record precisely what is broken and
      what fixing it needs.
- [ ] **Step 3: Commit** — `Put the caret where a bidi reader expects it`

---

## Closing

- [ ] Full suite; GPU alone; fmt; clippy.
- [ ] **Perform the acceptance sentence by hand**, in full.
- [ ] Update `ROADMAP.md`: tick what is done, and for anything still short, say
      what would close it.
