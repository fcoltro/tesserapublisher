# Milestone 1.5 Phase C-iii — The Chrome Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every command reachable, every mode selectable, and the document's state readable at a glance — and in doing so close the four gaps C-i and C-ii left recorded as partial.

**Architecture:** The command palette is a fuzzy filter over a list of named actions, each of which resolves to work the application already does. Both the palette and the menus drive that one list, so a command cannot exist in one and not the other.

**Tech Stack:** Rust 2024, egui 0.35, kurbo. No new dependencies.

**Spec:** [`docs/superpowers/specs/2026-09-03-instrument-milestone-design.md`](../specs/2026-09-03-instrument-milestone-design.md) — D3.

## Scope: this plan also closes four recorded gaps

| Gap | Where it closes |
|---|---|
| **C6** — only "align to selection" reachable | Task 1, the action list |
| **C7** — no flip or rotate on the canvas toolbar; text labels | Tasks 1 and 5 |
| **C8** — a placed guide cannot be moved or deleted | Task 6 |
| **C9** — Bleed and Slug unreachable; no clipping | Tasks 2 and 3 |

## Global Constraints

- `unsafe_code = "forbid"`. Document units are **points**.
- **No format version bump.**
- Every mutation goes through `Command`, or carries an `undo-bracketed:` marker.
- One completed gesture is **one** undo entry.
- Tests land in the same commit. `cargo clippy --workspace --all-targets -- -D warnings` clean before each.
- Single-crate test commands (`-p`).

---

### Task 1: One list of named actions

The list both the palette and the menus read, so a command cannot appear in one and be missing from the other.

**Files:** create `crates/tessera_ui/src/actions.rs`; modify `lib.rs`.

**Produces:**
- `Action` — `{ name: &'static str, shortcut: Option<&'static str>, group: Group, run: Run }`
- `Group::{ File, Edit, Object, Align, View, Tool }`
- `Run` — an enum naming work the application already does, so this module adds no behaviour
- `actions::all() -> &'static [Action]`
- `actions::matches(query: &str, name: &str) -> bool` — subsequence, case-insensitive

- [ ] **Step 1: Write the failing tests** — every name is unique and non-empty; every group is represented; the filter matches a subsequence (`"agl"` finds "Align left"), rejects a non-subsequence, and matches everything on an empty query.
- [ ] **Step 2: Run to verify they fail.** `cargo test -p tessera_ui actions::`
- [ ] **Step 3: Implement**, including the six align edges **against all four targets** — which is what makes C6 reachable — and flip and rotate 90°, which is what C7 was missing.
- [ ] **Step 4: Commit** — `Name every command once, for the palette and the menus both`

---

### Task 2: The menus

**Files:** modify `crates/tessera_ui/src/view/mod.rs`.

Layout, Type, View and Window join File, Edit and Object, each built from `actions::all()` filtered by group. **A menu entry for an unbuilt feature is a lie the previous codebase told often**, so a group with no actions yet gets no menu.

View carries the four screen modes, which is what makes Bleed and Slug reachable and closes half of C9.

- [ ] **Step 1: Write the failing test** — every action in the list belongs to a group that has a menu, so nothing can be added to the list and be unreachable.
- [ ] **Steps 2–4:** implement, run, commit — `Build the menus from the same list the palette reads`

---

### Task 3: Clip the document to the revealed rectangle

The other half of C9. Preview must show the trim *as it will print*, not merely hide the furniture.

**Files:** modify `crates/tessera_render/src/scene.rs`, `crates/tessera_ui/src/view/viewport.rs`.

- [ ] **Step 1: Write the failing test** — building a scene in Preview with an object outside the trim produces a different encoding from one in Normal, so the clip is really applied.
- [ ] **Step 2–4:** push a Vello clip layer around the revealed rect when the mode is a printing one; run; commit — `Clip the page to what each screen mode reveals`

---

### Task 4: The command palette

**Files:** create `crates/tessera_ui/src/view/palette.rs`; modify `view/mod.rs`.

`Ctrl`/`Cmd`+`K` opens a fuzzy-filtered list showing each command's shortcut beside it. D3: this is close to free because the list exists, and it teaches shortcuts as a side effect of being used.

- [ ] **Step 1: Write the failing tests** — a query narrows the list; the first match is selected; `Escape` closes without running anything; an empty query lists everything.
- [ ] **Steps 2–4:** implement, run, commit — `Add the command palette`

---

### Task 5: The icon set

**Files:** modify `crates/tessera_ui/src/icons.rs`, `crates/tessera_ui/src/view/canvas_toolbar.rs`.

Grow the Lucide set to cover the toolbar and the tool strip, and replace the toolbar's text labels. **Every added glyph must be real Lucide path data**, not invented: `every_icon_parses` proves a path parses, not that it draws the right thing, so a fabricated glyph would pass the suite and be wrong on screen. Add only glyphs whose data can be transcribed accurately; leave the rest as labels and say so.

- [ ] **Step 1–4:** add, run, commit — `Grow the icon set to cover the canvas toolbar`

---

### Task 6: Move and delete a placed guide

Closes C8.

**Files:** modify `crates/tessera_ui/src/view/viewport.rs`.

- [ ] **Step 1: Write the failing test** — hit-testing a guide finds it within a few screen pixels and not beyond.
- [ ] **Step 2–4:** dragging a guide is a bracketed gesture — history recorded on press, previewed live, one `MoveGuide` on release; dragged onto a ruler it is removed. Mark the live write `undo-bracketed:`. Run; commit — `Let a placed guide be moved and thrown away`

---

### Task 7: The status bar

**Files:** modify `crates/tessera_ui/src/view/panels.rs`.

Zoom control (a percentage that can be typed, plus fit-page) and a page navigator. The preflight indicator belongs to milestone 6 and is not built here.

- [ ] **Step 1–4:** implement with tests on the zoom parsing, run, commit — `Give the status bar a zoom control and a page navigator`

---

## Closing the plan

- [ ] Full non-GPU suite; clippy at `-D warnings`.
- [ ] **Perform the sentence, by hand:**

> Press `Ctrl`+`K`, type "algn", and align the selection left. Open the View menu and switch to Bleed; the page shows its bleed and nothing else. Back to Normal. Drag a guide onto the page, move it, then drag it onto a ruler and watch it go. Type 200 into the zoom field and see the canvas follow.

- [ ] Tick C10–C13 and re-check C6–C9, recording anything still partial.
- [ ] **Milestone 1.5 is then code complete.** Three hand checks remain owed — milestone 0's sentence, phase B's, and phase C's.
