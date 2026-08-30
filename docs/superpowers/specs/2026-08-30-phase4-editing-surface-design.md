# Phase 4 (core): The Editing Surface

Status: approved 2026-08-30
Scope decision: Phase 4.1 (state sync), 4.3 (property inspector), 4.4 (layers &
pages panels), plus the backend mutation commands they require.
Deferred by decision: 4.2 drag-to-float docking + workspace serialization, and
4.5 native menu bar.

## Problem

The backend finished Phase 3 well ahead of the frontend. `src-tauri/src/lib.rs`
exposes 46 IPC commands covering spreads, master pages, text threading,
snapping, and the baseline grid, but `src/routes/+page.svelte` is a single
1717-line file whose only editing affordance is a form that spawns a frame.

More precisely, the ECS already stores every property a publishing app needs to
edit — `Style { fill_color, stroke_color, stroke_width, opacity }`,
`TextContent { font_size, line_height, align, font_family, font_weight }`, and
`Layer { is_visible, is_locked, z_index }` — and **no command mutates any of
them**. The entire command surface is geometry, camera, and structure. A user
can move a frame but cannot change its colour, its typeface, or its stacking
order.

Phase 4 is therefore mostly an exposure problem, not a new-capability problem.

## Design

### 1. Backend: the mutation command surface

New commands follow the live-update / commit split that the geometry commands
already established (`set_frame_geometry` streams during a drag;
`commit_frame_geometry` records one undoable action on release,
`crates/core/src/lib.rs:1048` and `:1066`). Reusing that shape is what keeps a
scrub from 12pt to 60pt out of the undo stack as 48 separate entries.

**Style** — `get_frame_style`, `set_frame_style` (live, no history),
`commit_frame_style { before, after }`. The history variant already exists:
`HistoryAction::UpdateStyle` is defined at `crates/core/src/history.rs:38` and
is currently dead code with no producer. These commands are its first caller.

**Text** — `get_frame_text`, `set_frame_text` (live),
`commit_frame_text { before, after }`. This needs one new variant,
`HistoryAction::UpdateText { entity_index, old_text, new_text }`, added
alongside `UpdateStyle` with matching inverse/forward arms.
`EntitySnapshotData` already carries `text_content`, so spawn/despawn undo
already round-trips text correctly.

**Structure** — `set_layer_visibility`, `set_layer_locked`,
`set_frame_z_index`, `rename_frame`, `delete_frame`.

No renderer work is required for visibility: `crates/renderer/src/scene.rs:318`
already skips layers whose `is_visible` is false. The eye toggle needs only a
setter.

Each `AppState` method bumps the scene revision so the existing
`get_scene_revision` change-check stays truthful.

### 2. Frontend: decompose the monolith

`src/lib/` is created (it does not exist today):

- `ipc.ts` — typed wrappers over `invoke` plus every DTO type in one place.
  These types are currently inline in the page component and would otherwise be
  duplicated across every new panel.
- `state.svelte.ts` — the shared runes store holding selection, active tool,
  camera, history status, and document tree, with a single `invalidate()`
  entry point.
- `actions/scrub.ts` — click-and-drag-on-a-label numeric scrubbing, as a Svelte
  action so every numeric field gets it for free.

`src/lib/components/`:

- `AppShell.svelte` — the fixed dock. Tool rail, canvas, inspector column, with
  a pages strip below and resizable splitters. Panels do not float; slots are
  fixed. This keeps the panel-over-WebGPU-canvas z-index problem trivial and
  leaves room to layer true docking on later without rework.
- `Viewport.svelte` — the canvas plus all mouse and keyboard gesture handling,
  moved out of the page **verbatim**.
- `Inspector.svelte` — a `$derived` switch on the selected frame's
  `frame_type`, composing `TransformSection`, `VectorSection` (fill, stroke,
  stroke width, opacity), and `TypographySection` (family, weight, size,
  leading, alignment, baseline snap). Typography renders only for `Text`
  frames.
- `LayersPanel.svelte` — the tree from `query_document_tree`, with eye and
  padlock toggles and drag-and-drop reordering.
- `PagesPanel.svelte` — the spread grid, add/remove, and drag a master page
  onto a page to apply it.
- `NumberField.svelte` — label, scrub action, and input; the workhorse of both
  inspector sections.

`+page.svelte` shrinks to a shell that mounts `AppShell`.

### 3. Data flow

The current model is pull-only: every action calls `fetchScene()`, which
re-pulls four commands regardless of what changed, and `src-tauri` never calls
`emit` once.

This keeps pull as the base but stops the blanket re-pull. Mutation commands
emit a `document-changed` event carrying the new scene revision;
`state.svelte.ts` listens inside an `$effect` and re-pulls the document tree
only when the received revision differs from the one it last stored.

Inspector fields write on every keystroke or scrub tick via `set_*` (cheap, no
history) and call `commit_*` once on blur or pointer-up, passing the
before-value captured when the gesture began.

### 4. Verification

- `cargo test` — unit tests for each new `AppState` setter and an undo/redo
  round-trip for `UpdateStyle` and `UpdateText`. Core tests live inline at
  `crates/core/src/lib.rs:1393`; new tests follow that placement.
- `npm run check` — `svelte-check` over the new components and modules.

The repository has no frontend test runner (only `svelte-check`), and this work
does not add one.

## Risk

Lifting the viewport gesture code is the one genuinely dangerous step: roughly
400 lines of interdependent mouse, drag, snap, and pan state. It moves verbatim
into `Viewport.svelte` in its own commit, before any other frontend change, so
a regression there stays bisectable.

## Out of scope

Drag-to-float docking, workspace layout serialization to JSON, and the native
Tauri menu bar. All three remain in the Phase 4 plan document for a later step.
