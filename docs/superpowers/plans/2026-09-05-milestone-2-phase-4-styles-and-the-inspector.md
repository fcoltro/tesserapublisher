# Milestone 2 Phase 4 — Styles and the Inspector Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Select a word, make it bold, and see it — then define a paragraph style, apply it to several paragraphs, change the style, and watch every one update.

**Architecture:** Style tables live on the document; the cascade resolves through a trait so `tessera_text` never learns what a document is. Applying formatting is one command over a text range, splitting runs at its edges.

**Spec:** [milestone 2 typography](../specs/2026-09-05-milestone-2-typography-design.md) — D2, D2a, D3.

**Preceded by:** phases 1 (runs), 2 (format 6), 3 (shaping runs).

## The one structural problem to solve first

The shaper needs a **resolved** format per run. The style tables live on the
document. `tessera_text` cannot see `tessera_document` — the dependency points
the other way, and its module doc says that isolation is the point.

So the cascade resolves through a trait declared in `tessera_text` and
implemented by `Document`:

```rust
pub trait Styles {
    fn character(&self, id: CharacterStyleId) -> Option<&CharacterFormat>;
    fn paragraph(&self, id: ParagraphStyleId) -> Option<&ParagraphFormat>;
    fn document_default(&self) -> CharacterFormat;
}
```

`tessera_layout` already holds both a `Document` and a `Shaper`, so it is the
one place that can hand the second to the first. A `NoStyles` implementation
serves the tests and the caret, which have a story but no document.

*Rejected: resolving in `tessera_layout` and passing spans to the shaper.* It
works, but it gives `Shaper` a second entry point and leaves the caret — which
shapes through `layout()` — resolving differently from the renderer. One path
or the formatting on screen and the caret's idea of it drift apart.

## Global Constraints

- `unsafe_code = "forbid"`. Document units are **points**.
- **Format version 7, one bump**, carrying the style tables, the document
  default, and the retirement of `Story::style`.
- Every mutation goes through `Command`; one completed edit is one undo entry.
- Tests land in the same commit. `cargo clippy --workspace --all-targets -- -D warnings` clean.

---

### Task 1: The style tables and the cascade

**Files:** `crates/tessera_text/src/story.rs`, `crates/tessera_document/src/nodes.rs`, `crates/tessera_document/src/document.rs`

**Produces:**
- `Styles` trait and `NoStyles`
- `CharacterStyle { name, format }`, `ParagraphStyle { name, format, character }`
- `Document::character_styles`, `Document::paragraph_styles`, `Document::text_default`
- `Story::resolve_run(&self, run, paragraph, styles: &dyn Styles) -> CharacterFormat`

Resolution order, fixed here and nowhere else:

```text
document default -> paragraph style -> paragraph local -> character style -> run local
```

- [ ] **Step 1: Write the failing tests** — each level overrides the one before it; a run naming a style that has been deleted falls back rather than panicking; `NoStyles` resolves to local-over-default.
- [ ] **Step 2–4:** implement, run, commit — `Resolve a run through all four levels`

---

### Task 2: Format version 7

**Files:** `crates/tessera_document/src/format/mod.rs`, `crates/tessera_document/tests/round_trip.rs`

The migration folds each story's `style` into every run's local format — `run.local` **over** the style, so a run that already states a size keeps it — and then the field goes.

- [ ] **Step 1: Write the failing test** with a hand-built version-6 archive. **Not `rewrite_version_for_test`**: it round-trips through the current model, which is how three tests in phase 2 passed while testing nothing.
- [ ] **Step 2–4:** implement, run, commit — `Move the format to version 7 and retire the story's one style`

---

### Task 3: Applying formatting to a range

**Files:** `crates/tessera_text/src/story.rs`, `crates/tessera_ui/src/command.rs`

**Produces:**
- `Story::apply_character_format(&mut self, range, format)` — splits runs at
  the range's edges, merges the format into every run inside it, then folds
  identical neighbours
- `Command::SetCharacterFormat { story, range, format }`
- `Command::SetParagraphFormat { story, range, format }`

- [ ] **Step 1: Write the failing tests** — formatting a middle range splits one run into three; formatting the whole story leaves one run; the invariant holds after every application; a property test over generated ranges.
- [ ] **Step 2–4:** implement, run, commit — `Make a word bold`

---

### Task 4: Font enumeration

**Files:** `crates/tessera_text/src/shape.rs`

`Shaper::families() -> &[String]`, from `fontique::Collection::family_names()`, sorted and cached once. A family a document names but the system lacks is substituted by parley already; **marking it** is the visible half and belongs here.

- [ ] **Step 1–4:** implement with a test that the list is non-empty and sorted, run, commit — `List the fonts this system has`

---

### Task 5: The typography inspector

**Files:** `crates/tessera_ui/src/view/panels.rs`

The `Text` section gains family, size, weight, italic, leading, tracking; then alignment and indents; then the two style pickers.

**What the controls act on** is the decision worth stating: a text selection if there is one, otherwise the whole story. That is InDesign's rule — select the frame and formatting applies to all of its text — and it means the section is useful before a caret exists.

- [ ] **Step 1–4:** implement, run, commit — `Give typography a panel`

---

## Closing the phase

- [ ] Full non-GPU suite; GPU alone; clippy at `-D warnings`.
- [ ] **Perform the sentence, by hand:**

> Type a sentence. Select one word and make it bold; watch only that word thicken. Set the frame's text in a chosen family at a chosen size and leading. Define a paragraph style, apply it to two paragraphs, change the style's size, and watch both follow.

- [ ] Tick milestone 2's items, recording anything partial.
