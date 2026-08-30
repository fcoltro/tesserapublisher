# Phase 4 Core Editing Surface — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make Tessera's documents editable — expose the `Style`, `TextContent`,
and `Layer` data the ECS already holds through a mutation command surface and a
decomposed Svelte panel UI.

**Architecture:** New backend commands mirror the existing live-update / commit
split (`set_*` streams without history, `commit_*` records one undoable action
on gesture end). The 1717-line `+page.svelte` decomposes into `src/lib/`
modules and focused components behind a fixed dock shell. Panels invalidate off
a scene-revision event instead of re-pulling everything after every call.

**Tech Stack:** Rust, bevy_ecs 0.15, im 15 (history), Tauri v2, Svelte 5
(runes), TypeScript, Vite.

**Spec:** `docs/superpowers/specs/2026-08-30-phase4-editing-surface-design.md`

## Global Constraints

- Rust edition 2021; workspace crates `tessera-core`, `tessera-renderer`, `src-tauri`.
- Core unit tests live inline in a `mod tests` block at the bottom of each core
  module (`crates/core/src/lib.rs:1393` is the existing block).
- Every new `AppState` mutator calls `self.increment_scene_revision()` after
  dropping its world lock.
- Every new `#[tauri::command]` must be added to the `generate_handler!` list at
  `src-tauri/src/lib.rs:573`.
- `commit_*` commands drop no-op gestures (old == new pushes nothing), matching
  `commit_frame_geometry`.
- Svelte 5 runes only — `$state`, `$derived`, `$effect`. No stores API.
- Verification is `cargo test` and `npm run check`. No frontend test runner
  exists; do not add one.

---

### Task 1: Style mutation commands

**Files:**
- Modify: `crates/core/src/lib.rs` (AppState impl, near `set_frame_geometry:1048`)
- Modify: `src-tauri/src/lib.rs` (commands + handler list)
- Test: `crates/core/src/lib.rs` `mod tests`

**Interfaces:**
- Consumes: existing `HistoryAction::UpdateStyle` (`crates/core/src/history.rs:38`, currently no producer)
- Produces:
  - `AppState::get_frame_style(&self, entity_index: u32) -> Result<Style, String>`
  - `AppState::set_frame_style(&self, entity_index: u32, style: Style) -> Result<(), String>`
  - `AppState::commit_frame_style(&self, entity_index: u32, old: Style, new: Style) -> Result<HistoryStatus, String>`
  - commands `get_frame_style`, `set_frame_style`, `commit_frame_style`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn set_frame_style_updates_without_history() {
    let state = AppState::new();
    let id = spawn_test_frame(&state);
    let mut style = state.get_frame_style(id).unwrap();
    style.fill_color = [1.0, 0.0, 0.0, 1.0];
    state.set_frame_style(id, style).unwrap();
    assert_eq!(state.get_frame_style(id).unwrap().fill_color, [1.0, 0.0, 0.0, 1.0]);
    assert!(!state.get_history_status().unwrap().can_undo);
}

#[test]
fn commit_frame_style_is_undoable() {
    let state = AppState::new();
    let id = spawn_test_frame(&state);
    let old = state.get_frame_style(id).unwrap();
    let new = Style { fill_color: [0.0, 1.0, 0.0, 1.0], ..old };
    state.commit_frame_style(id, old, new).unwrap();
    state.undo().unwrap();
    assert_eq!(state.get_frame_style(id).unwrap().fill_color, old.fill_color);
}

#[test]
fn commit_frame_style_drops_noop() {
    let state = AppState::new();
    let id = spawn_test_frame(&state);
    let s = state.get_frame_style(id).unwrap();
    state.commit_frame_style(id, s, s).unwrap();
    assert!(!state.get_history_status().unwrap().can_undo);
}
```

Reuse the existing test helper that spawns a frame; if none exists, add
`fn spawn_test_frame(state: &AppState) -> u32` to the tests module.

- [ ] **Step 2: Run tests, verify they fail**

Run: `cargo test -p tessera-core style`
Expected: FAIL — no method `get_frame_style`.

- [ ] **Step 3: Implement the three AppState methods**

Mirror `set_frame_geometry` / `commit_frame_geometry` exactly: read lock for
the getter, write lock + `increment_scene_revision()` for the setter, no-op
guard + `HistoryAction::UpdateStyle` push for the commit.

- [ ] **Step 4: Add the three Tauri commands and register them**

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test -p tessera-core` then `cargo check -p tessera-publish`
Expected: PASS.

- [ ] **Step 6: Commit** — `feat: add frame style mutation commands`

---

### Task 2: Text mutation commands

**Files:**
- Modify: `crates/core/src/history.rs` (new variant + inverse/forward arms)
- Modify: `crates/core/src/lib.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
  - `HistoryAction::UpdateText { entity_index: u32, old_text: TextContent, new_text: TextContent }`
  - `AppState::get_frame_text(&self, entity_index: u32) -> Result<TextContent, String>`
  - `AppState::set_frame_text(&self, entity_index: u32, text: TextContent) -> Result<(), String>`
  - `AppState::commit_frame_text(&self, entity_index: u32, old: TextContent, new: TextContent) -> Result<HistoryStatus, String>`
  - commands `get_frame_text`, `set_frame_text`, `commit_frame_text`

Note: `set_frame_text` must re-run text layout / bounding box the same way
`set_frame_baseline_snap` (`crates/core/src/lib.rs:1158`) does — follow that
method's handling rather than only writing the component.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn commit_frame_text_round_trips_through_undo_redo() {
    let state = AppState::new();
    let id = spawn_test_text_frame(&state);
    let old = state.get_frame_text(id).unwrap();
    let new = TextContent { font_size: 42.0, text: "Changed".into(), ..old.clone() };
    state.commit_frame_text(id, old.clone(), new.clone()).unwrap();
    assert_eq!(state.get_frame_text(id).unwrap().font_size, 42.0);
    state.undo().unwrap();
    assert_eq!(state.get_frame_text(id).unwrap(), old);
    state.redo().unwrap();
    assert_eq!(state.get_frame_text(id).unwrap(), new);
}
```

- [ ] **Step 2: Run, verify fail** — `cargo test -p tessera-core text`
- [ ] **Step 3: Add the `UpdateText` variant** with arms in both
      `apply_inverse_action` (`history.rs:105`) and `apply_forward_action` (`:178`)
- [ ] **Step 4: Implement the three AppState methods**
- [ ] **Step 5: Add and register the three commands**
- [ ] **Step 6: Run tests, verify pass** — `cargo test -p tessera-core`
- [ ] **Step 7: Commit** — `feat: add frame text mutation commands`

---

### Task 3: Structure commands (layers, order, rename, delete)

**Files:**
- Modify: `crates/core/src/lib.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
  - `AppState::set_layer_visibility(&self, layer_index: u32, visible: bool) -> Result<(), String>`
  - `AppState::set_layer_locked(&self, layer_index: u32, locked: bool) -> Result<(), String>`
  - `AppState::set_frame_z_index(&self, entity_index: u32, z: i32) -> Result<(), String>`
  - `AppState::rename_frame(&self, entity_index: u32, name: String) -> Result<(), String>`
  - `AppState::delete_frame(&self, entity_index: u32) -> Result<HistoryStatus, String>`
  - matching commands, same names

`delete_frame` captures an `EntitySnapshotData` before despawning and pushes
`HistoryAction::DespawnFrame`, so deletion is undoable. Reuse whatever helper
`spawn_frame` uses to build that snapshot.

Renderer needs no change for visibility: `crates/renderer/src/scene.rs:318`
already skips layers where `is_visible` is false.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn hidden_layer_is_flagged_in_the_document_tree() {
    let state = AppState::new();
    let tree = state.get_document_tree().unwrap();
    let layer_id = tree.pages[0].layers[0].id;
    state.set_layer_visibility(layer_id, false).unwrap();
    let tree = state.get_document_tree().unwrap();
    assert!(!tree.pages[0].layers[0].is_visible);
}

#[test]
fn delete_frame_is_undoable() {
    let state = AppState::new();
    let id = spawn_test_frame(&state);
    state.delete_frame(id).unwrap();
    assert!(state.get_frame_geometry(id).is_err());
    state.undo().unwrap();
    assert!(state.get_frame_geometry(id).is_ok());
}

#[test]
fn set_frame_z_index_reorders() {
    let state = AppState::new();
    let id = spawn_test_frame(&state);
    state.set_frame_z_index(id, 7).unwrap();
    let tree = state.get_document_tree().unwrap();
    let frame = tree.pages[0].layers[0].frames.iter().find(|f| f.id == id).unwrap();
    assert_eq!(frame.z_index, 7);
}
```

- [ ] **Step 2: Run, verify fail** — `cargo test -p tessera-core`
- [ ] **Step 3: Implement the five AppState methods**
- [ ] **Step 4: Add and register the five commands**
- [ ] **Step 5: Run tests, verify pass**
- [ ] **Step 6: Commit** — `feat: add layer and frame structure commands`

---

### Task 4: Frontend foundation and verbatim viewport lift

This is the risky task. It must change **no behavior at all** — it only moves
code. Do it alone, commit it alone, so any regression bisects cleanly.

**Files:**
- Create: `src/lib/ipc.ts` — every DTO type + typed `invoke` wrappers
- Create: `src/lib/state.svelte.ts` — shared runes store
- Create: `src/lib/components/AppShell.svelte` — fixed dock grid + splitters
- Create: `src/lib/components/Viewport.svelte` — canvas + gestures, moved verbatim
- Modify: `src/routes/+page.svelte` — shrinks to mounting `AppShell`

**Interfaces:**
- Produces (`ipc.ts`): types `Camera`, `HistoryStatus`, `FrameGeometry`,
  `DocumentTreeSnapshot`, `PageNode`, `LayerNode`, `FrameNode`, `Style`,
  `TextContent`, `PagePlacement`, `DocumentSettings`, `BaselineGrid`,
  `RendererInfo`; wrapper functions named exactly after their commands.
- Produces (`state.svelte.ts`): a `studio` object with `$state` fields
  `selectedEntityId`, `activeTool`, `camera`, `history`, `tree`,
  `documentSettings`, `pages`, and methods `invalidate()`, `select(id)`.

- [ ] **Step 1: Extract every `interface`/`type` and `invoke` call from
      `+page.svelte` into `ipc.ts`** — names unchanged.
- [ ] **Step 2: Create `state.svelte.ts`** holding the state currently declared
      at `+page.svelte:152-200`, with `invalidate()` doing what `fetchScene()`
      does today.
- [ ] **Step 3: Move canvas markup + all gesture handlers
      (`+page.svelte:238-888` handlers, `:1054` canvas container) into
      `Viewport.svelte` unchanged.**
- [ ] **Step 4: Create `AppShell.svelte`** — CSS grid: tool rail (left),
      `<Viewport />` (center), inspector column (right), pages strip (bottom).
      Splitters resize columns; slots are fixed. Inspector and pages slots are
      empty placeholders in this task.
- [ ] **Step 5: Reduce `+page.svelte` to mounting `AppShell`.**
- [ ] **Step 6: Verify** — `npm run check` passes, and `npm run tauri dev`
      still pans, zooms, selects, drags, resizes, snaps, and undoes exactly as
      before. This is a manual smoke check; there is no frontend test runner.
- [ ] **Step 7: Commit** — `refactor: decompose page into shell, viewport, and lib modules`

---

### Task 5: The property inspector

**Files:**
- Create: `src/lib/actions/scrub.ts`
- Create: `src/lib/components/NumberField.svelte`
- Create: `src/lib/components/Inspector.svelte`
- Create: `src/lib/components/inspector/TransformSection.svelte`
- Create: `src/lib/components/inspector/VectorSection.svelte`
- Create: `src/lib/components/inspector/TypographySection.svelte`
- Modify: `src/lib/components/AppShell.svelte` (fill the inspector slot)

**Interfaces:**
- Consumes: Task 1 and Task 2 commands via `ipc.ts`.
- Produces: `scrub` — a Svelte action `(node, { onDelta, step }) => ...` that
  turns horizontal pointer drag into value deltas and reports gesture start/end
  so the caller knows when to `commit_*`.

Gesture contract, applied by every field: capture `before` on gesture start,
call `set_*` on every tick, call `commit_*(before, after)` once on pointerup or
blur. This is what keeps one scrub as one undo entry.

- [ ] **Step 1: Write `scrub.ts`** — pointerdown/move/up with pointer capture.
- [ ] **Step 2: Write `NumberField.svelte`** — label + `use:scrub` + number
      input, emitting `oninput` / `oncommit`.
- [ ] **Step 3: Write `VectorSection.svelte`** — fill colour, stroke colour,
      stroke width, opacity, wired to `get/set/commit_frame_style`.
- [ ] **Step 4: Write `TypographySection.svelte`** — font family, weight, size,
      leading, alignment, baseline-snap toggle, wired to
      `get/set/commit_frame_text` and the existing `set_frame_baseline_snap`.
- [ ] **Step 5: Write `TransformSection.svelte`** — X, Y, W, H via the existing
      `get/set/commit_frame_geometry`.
- [ ] **Step 6: Write `Inspector.svelte`** — `$derived` on the selected frame's
      `frame_type`; Typography renders only for `Text`, Vector for all types.
- [ ] **Step 7: Verify** — `npm run check`; manually confirm a font-size scrub
      produces exactly one undo entry.
- [ ] **Step 8: Commit** — `feat: add context-aware property inspector`

---

### Task 6: Layers and Pages panels

**Files:**
- Create: `src/lib/components/LayersPanel.svelte`
- Create: `src/lib/components/PagesPanel.svelte`
- Modify: `src/lib/components/AppShell.svelte`

**Interfaces:** consumes Task 3 commands plus existing `query_document_tree`,
`get_page_placements`, `add_page`, `remove_page`, `list_master_pages`,
`apply_master_to_page`.

- [ ] **Step 1: `LayersPanel.svelte`** — render `tree.pages[].layers[].frames[]`,
      eye toggle → `set_layer_visibility`, padlock → `set_layer_locked`,
      click a frame row → `studio.select(id)`, HTML5 drag-and-drop on frame
      rows → `set_frame_z_index`, double-click name → `rename_frame`,
      delete key → `delete_frame`.
- [ ] **Step 2: `PagesPanel.svelte`** — spread grid from `get_page_placements`,
      add/remove buttons, drag a master from `list_master_pages` onto a page →
      `apply_master_to_page`.
- [ ] **Step 3: Mount both in `AppShell`** (layers above inspector, pages in the
      bottom strip).
- [ ] **Step 4: Verify** — `npm run check`; hiding a layer visibly removes its
      frames from the canvas (proves the `scene.rs:318` path).
- [ ] **Step 5: Commit** — `feat: add layers and pages panels`

---

### Task 7: Revision-driven invalidation

**Files:**
- Modify: `src-tauri/src/lib.rs` (emit from mutation commands)
- Modify: `src/lib/state.svelte.ts` (listen)

**Interfaces:**
- Produces: Tauri event `document-changed` with payload
  `{ revision: u64 }`, emitted by every command that bumps the scene revision.

- [ ] **Step 1: Emit `document-changed`** from mutation commands using
      `tauri::Emitter` on the app handle, carrying the post-mutation revision.
- [ ] **Step 2: Listen in `state.svelte.ts`** inside an `$effect`, re-pulling
      the document tree only when the received revision differs from the last
      stored one; unlisten on teardown.
- [ ] **Step 3: Remove the blanket post-action `invalidate()` calls** that the
      event now covers.
- [ ] **Step 4: Verify** — `npm run check`, `cargo check`; panels still update
      after every edit.
- [ ] **Step 5: Commit** — `feat: drive panel refresh from scene revision events`

---

## Self-Review

**Spec coverage:** Backend style commands → Task 1. Text commands + `UpdateText`
→ Task 2. Structure commands → Task 3. `src/lib` modules, `AppShell`,
verbatim `Viewport` lift → Task 4. Inspector and its sections, `scrub`,
`NumberField` → Task 5. Layers and Pages panels → Task 6. Event-driven data
flow → Task 7. Verification via `cargo test` / `npm run check` → constraints
plus each task's verify step. No spec section is unassigned.

**Type consistency:** `Style` and `TextContent` are the ECS component types
from `crates/core/src/components.rs`, serialized as-is over IPC — the same
names are used in `ipc.ts` (Task 4), consumed by Tasks 5 and 6. `studio` and
its `invalidate()` / `select()` methods are named identically in Tasks 4, 6,
and 7.
