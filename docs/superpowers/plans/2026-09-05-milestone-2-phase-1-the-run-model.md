# Milestone 2 Phase 1 — The Run Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a story hold more than one formatting, and carry that formatting correctly through every edit — as pure data, before any interface or file format depends on it.

**Architecture:** One contiguous `String` with sorted, non-overlapping, gap-free run lists over it (D1). Every format field is `Option`, so a run holds a reference and an override rather than a resolved copy (D2). All of it headless.

**Spec:** [milestone 2 typography](../specs/2026-09-05-milestone-2-typography-design.md) — D1, D2, D2a, D4.

## Global Constraints

- `unsafe_code = "forbid"`. Document units are **points**.
- **No format version bump in this phase.** `Story` gains fields with
  `serde(default)`; the bump and its migration are phase 2.
- Nothing in this phase touches the interface or the renderer.
- Tests land in the same commit. `cargo clippy --workspace --all-targets -- -D warnings` clean.

## The invariant, stated once

For a story of text length `n`, both `runs` and `paragraphs` are:

- **sorted** by start,
- **non-overlapping**,
- **gap-free**, covering exactly `[0, n)`,
- and **non-empty** unless `n == 0`.

Every operation below preserves it, and a property test over generated edit
sequences is what proves they do. A run list that drifts out of step with the
text is corruption rather than a glitch, and its symptom shows up far from its
cause.

---

### Task 1: The format types

**Files:** modify `crates/tessera_text/src/story.rs`, `crates/tessera_text/Cargo.toml`

**Produces:**
- `Case::{ Normal, Upper, Lower, SmallCaps }`
- `Alignment::{ Left, Centre, Right, Justify }`
- `CharacterFormat` — every field `Option`, plus `Default`
- `ParagraphFormat` — every field `Option`, plus a nested `CharacterFormat`
- `CharacterStyleId`, `ParagraphStyleId` — slotmap key types, defined here
  because the id is part of the story's vocabulary even though the arena
  lives on the document
- `CharacterFormat::over(&self, base: &CharacterFormat) -> CharacterFormat` —
  the cascade step: `self`'s `Some` values win, `None` inherits

- [ ] **Step 1: Write the failing tests** — an empty format inherits everything; a full one overrides everything; `over` is associative across three levels, which is what makes a four-level cascade safe to fold in any order.
- [ ] **Step 2: Run to verify they fail.** `cargo test -p tessera_text story::`
- [ ] **Step 3: Implement**, adding `slotmap` to the crate's dependencies.
- [ ] **Step 4: Run, then commit** — `Give text formatting somewhere to be partial`

---

### Task 2: The run lists and their invariant

**Files:** modify `crates/tessera_text/src/story.rs`

**Produces:**
- `Run { range: Range<usize>, style: Option<CharacterStyleId>, local: CharacterFormat }`
- `ParagraphRun { range, style: Option<ParagraphStyleId>, local: ParagraphFormat }`
- `Story::runs`, `Story::paragraphs` — both `#[serde(default)]`
- `Story::runs_are_sound(&self) -> bool` — the invariant, as one function
- `Story::run_at(&self, offset: usize) -> Option<&Run>`

- [ ] **Step 1: Write the failing tests** — a new story has exactly one run covering all of it; an empty story has none; `run_at` finds the run containing an offset and nothing past the end; `runs_are_sound` rejects a gap, an overlap, an unsorted pair and a list that stops short of the text.
- [ ] **Step 2–4:** implement, run, commit — `Let a story hold more than one formatting`

---

### Task 3: Edits carry the runs (D4)

The part most likely to harbour bugs, and the reason this phase exists before any interface.

**Files:** modify `crates/tessera_text/src/story.rs`

**Produces:**
- `Story::insert_text(&mut self, at: usize, text: &str)`
- `Story::delete_range(&mut self, range: Range<usize>)`
- `Story::split_run_at(&mut self, offset: usize) -> usize` — index of the run starting there
- `Story::merge_equal_neighbours(&mut self)`

Rules, each its own test:

- Inserting **inside** a run extends it.
- Inserting **at a boundary** joins the run to the **left** — what every editor does, and what makes typing after a bold word continue bold.
- Inserting into an empty story creates the one covering run.
- Deleting a range drops runs it wholly covers and clips the ones it straddles.
- Deleting everything leaves no runs, not one empty run.
- Adjacent runs that resolve identically merge, or the list grows without bound over a long session.

- [ ] **Step 1: Write the failing tests**, one per rule, plus a proptest: over a generated sequence of inserts and deletes, `runs_are_sound` holds at every step and the runs still cover exactly the text's length.
- [ ] **Step 2–4:** implement, run, commit — `Carry formatting through every edit`

---

### Task 4: Wire the buffer to it

`EditBuffer` already edits a story's text. It must go through the run-aware operations, or typing silently destroys the invariant.

**Files:** modify `crates/tessera_text/src/edit.rs`

- [ ] **Step 1: Write the failing test** — type into a story with two runs and the runs still cover the text; backspace across a run boundary and they still do.
- [ ] **Step 2–4:** route every mutation through task 3's operations, run the whole `tessera_text` suite, commit — `Keep the runs sound while the caret moves`

---

## Closing the phase

- [ ] Full non-GPU suite; clippy at `-D warnings`.
- [ ] **The phase has no sentence a person can perform** — it is all model, and that is what makes it phase 1. Its acceptance is that the property test holds and the existing text tests still pass unchanged.
- [ ] **Write the phase 2 plan:** format version 6 and its migration, turning each existing single-`TextStyle` story into one run and one paragraph.
