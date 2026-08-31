# Tessera Publisher — Roadmap

A professional desktop publishing application: an InDesign-class layout tool,
free and genuinely cross-platform. Linux is a first-class target, not an
afterthought — the absence of a serious DTP application on Linux is the reason
this project exists.

**Status legend:** `[x]` done and verified in the codebase · `[~]` partially
done, see note · `[ ]` not started

> Checkbox accuracy matters more than optimism. Before marking an item `[x]`,
> confirm it in the code. This file is the source of truth for what is built.

---

## Cross-Cutting Requirements

These apply to every phase and are not "done" until shipping.

- [~] **Cross-platform parity (Linux / Windows / macOS).** Renderer backends
  are selected per-platform in `crates/renderer` rather than defaulting to
  DX12, and `macos-private-api` is enabled for transparent-window compositing.
  Linux has not yet been booted or tested — no Wayland/X11 verification, no
  packaging. This is the project's defining requirement and needs a dedicated
  pass.
- [ ] **CI across all three platforms** — build and test matrix.
- [ ] **Packaging:** AppImage / Flatpak (Linux), MSI (Windows), DMG (macOS).
- [x] **Test discipline.** 156 tests across core and renderer, including
  GPU-backed render tests in `crates/renderer/tests/gpu_render.rs`.

---

# Phase 1: Core Foundation & Rendering — COMPLETE

## 1. Workspace Initialization
- [x] Initialize Tauri v2 project with Svelte 5 (Runes) and Vite.
- [x] Configure `Cargo.toml` with workspaces (`core`, `renderer`, `app`).
- [x] Set up basic IPC communication bridge for testing strings/JSON.

## 2. Document State Architecture (Rust)
- [x] Integrate `bevy_ecs` for document state.
- [x] Define core entities: `Document`, `Page`, `Layer`, `Frame`.
- [x] Define core components: `Transform`, `ZIndex`, `BoundingBox`.
- [x] Thread-safe state wrapper (`RwLock<World>`) in Tauri's managed state.
- [x] Immutable Undo/Redo stack using `im` (`crates/core/src/history.rs`).

## 3. The Rendering Engine (WebGPU + Vello)
- [x] Mount a full-screen canvas and bind the native surface.
- [x] Initialize the `vello` renderer in Rust.
- [x] Scene generator: compile ECS entities into a `vello::Scene`.
- [x] Render loop triggered by state change, not a blind 60 FPS tick.

## 4. Camera & Workspace Interaction
- [x] `Camera` component with pan X/Y and zoom (`crates/core/src/camera.rs`).
- [x] Affine screen-to-document coordinate mapping.
- [x] Mouse wheel / trackpad bound to camera IPC commands.
- [x] Selection by raycast against ECS bounding boxes.

---

# Phase 2: Typography & Vector Engine — COMPLETE (one deviation)

## 1. The Vector Primitives (Kurbo)
- [x] `kurbo` for 2D geometry, hit-testing, and bezier math.
- [x] `FrameType` covers `Rectangle`, `Ellipse`, `Line`, `Path`.
- [x] Click-and-drag creation driving `Size` / `Path` updates.
- [x] Custom shape rendering via `kurbo::BezPath` in the scene generator.
- [x] Precise bounding boxes from kurbo bounds, feeding mouse selection.

## 2. Text Shaping & Layout (Parley)
- [x] Linebender stack integrated: `parley`, `skrifa`, `fontique`.
- [x] `FontContext` and `LayoutContext` as shared resources.
- [x] `TextContent` component: string, size, weight, alignment, leading.
- [x] Text system builds a `parley` layout with resolved glyphs and breaks.
- [x] Positioned glyphs rendered into the Vello scene.

## 3. Canvas Interactions & Tooling
- [x] Tool state machine in Svelte: `Select`, `Rectangle`, `Ellipse`, `Text`.
- [~] **Deviation — active tool is not sent to Rust.** The frontend interprets
  the gesture itself and calls the specific command it wants. This is a
  deliberate simplification: it keeps gesture interpretation in one place
  rather than splitting it across the IPC boundary. Revisit only if the
  backend ever needs to vary behavior by tool.
- [x] Selection overlay drawn in Vello with corner anchor handles.
- [x] Entity translation by dragging.
- [x] Entity scaling by dragging handles, with live text reflow.

## 4. State & Performance Refinement
- [x] Gesture completion pushes one delta to the Undo/Redo stack.
- [x] Scene rebuilds are gated on a scene revision counter.

---

# Phase 3: Document Architecture & Layout Systems — COMPLETE (one gap)

## 1. Document Hierarchy & Spreads
- [x] ECS tree: `Document -> Spread -> Page -> Layer -> Frame`.
- [x] Facing-pages spread offsets on the infinite canvas.
- [x] Document settings: DPI, bleed, page size.
- [x] Pasteboard and physical page boundaries drawn as base layers.

## 2. Master Pages (Templates)
- [x] Separate ECS hierarchy for `MasterPage`.
- [x] Master items render behind a standard page's own entities.
- [x] Master overrides promote an item to a local, editable entity.

## 3. Text Threading (Linked Frames)
- [x] `TextThread` component with next/prev pointers.
- [x] Overflow passes the remaining story to the next frame in the chain.
- [x] Reflow cascades through the whole thread when a frame resizes.
- [ ] **Gap — thread indicator lines are never drawn.** The story model works,
  but nothing renders the bezier connectors between linked frames that
  InDesign shows on selection. Renderer work, in `crates/renderer/src/paint.rs`.

## 4. Grids, Guides, and Snapping
- [x] Margin and column guides (`PageGuides`, `crates/core/src/layout.rs:143`).
- [x] User-dragged ruler guides, horizontal and vertical.
- [x] Baseline grid with a per-frame snap toggle.
- [x] Snapping engine with a pixel-threshold lock to guides, grid, and bounds.

---

# Phase 4: The DTP Interface & Workspace (Svelte 5) — IN PROGRESS

The backend is far ahead of the UI. The ECS already stores every property this
phase needs to edit; almost none of it is reachable from the interface. See
`docs/superpowers/specs/2026-08-30-phase4-editing-surface-design.md` and
`docs/superpowers/plans/2026-08-30-phase4-editing-surface.md`.

**Backend prerequisites discovered during planning** — these must land first:

- [x] Mutation commands for `Style` (get/set/commit), wired to the existing
  `HistoryAction::UpdateStyle`, which until now had no producer.
- [x] Mutation commands for `TextContent` (get/set/commit) plus the new
  `HistoryAction::UpdateText` variant.
- [x] Structure commands: layer visibility, layer lock, frame z-index, rename,
  delete (undoable via `DespawnFrame`).
- [ ] **`TextContent` has no `tracking` field.** Required by the Typography
  Inspector below. Component change, then renderer support in `text.rs`.
- [ ] **`Style` has no `corner_radius` field.** Required by the Vector
  Inspector below. Component change, then renderer support in `paint.rs`.

## 1. State Synchronization (Svelte <-> Rust)
- [ ] Global Svelte `$state` mirroring selection and active tool.
- [ ] Tauri `listen` inside `$effect` for backend-initiated changes. Today the
  frontend is pull-only and `src-tauri` never calls `emit`.
- [ ] Debounced IPC for continuous inputs.

## 2. Dockable Panel Architecture — DEFERRED BY DECISION
Fixed, resizable dock slots ship first; true floating/docking is postponed
until the editing surface is proven. Revisit after Phase 4.4.
- [ ] Drag/drop/float/snap panel system.
- [ ] Workspaces serialized to JSON via Tauri `fs`.
- [x] Panels never trapped under the WebGPU canvas — solved by construction,
  since the native Vello surface sits behind a transparent webview.

## 3. The Context-Aware Property Inspector
- [ ] Control panel keyed on the selected entity's `frame_type` via `$derived`.
- [ ] **Typography Inspector** (Text frames only): family, style, size,
  leading, tracking, paragraph alignment.
- [ ] **Vector Inspector**: fill, stroke, stroke width, corner radius.
- [ ] Input scrubbing component — drag a number label to change its value.

## 4. Layers and Pages Panels
- [ ] **Layers Panel:** the ECS tree, with drag-and-drop reordering.
- [ ] Visibility (eye) and lock (padlock) toggles. The renderer already honors
  `Layer.is_visible` at `crates/renderer/src/scene.rs:318` — this needs only a
  setter command.
- [ ] **Pages Panel:** visual grid of spreads and master pages.
- [ ] Drag a master onto a page to apply it; drag to reorder pages.

## 5. Global Menus and Keyboard Shortcuts — DEFERRED BY DECISION
- [ ] Native OS menu bar (File, Edit, Layout, Type, Object, View, Window).
- [ ] Native menu clicks mapped to frontend events.
- [x] Keyboard shortcuts for tool switching (`V`, `T`, spacebar-to-pan) —
  already implemented in the existing page component.

---

# Phase 5: Asset Management & Color Engine — NOT STARTED

## 1. Image Linking & Proxy Rendering
- [ ] `ImageFrame` component for raster graphics (JPG, PNG, TIFF).
- [ ] Asset linking by absolute path, loaded dynamically — never embedded.
- [ ] Proxy generator: low-res cached preview for canvas rendering.
- [ ] Asset Manager panel: link status (Missing / Modified / OK), effective PPI.

## 2. Clipping Paths & Image Masking
- [ ] Clipping masks in Vello so vector shapes mask contained rasters.
- [ ] Select content separately from container; pan and scale within the frame.
- [ ] "Fit Content to Frame" and "Fill Frame Proportionally".

## 3. Professional Color Engine (CMYK & ICC Profiles)
- [ ] Integrate `lcms2` for accurate color conversion.
- [ ] Color components spanning RGB, CMYK, and alpha.
- [ ] Document-level ICC profiles (SWOP, FOGRA39) for on-screen print
  simulation.
- [ ] Swatches panel with global colors that re-render every user on edit.
- [ ] Spot colors that bypass CMYK separation.

## 4. Advanced Fills & Effects
- [ ] Linear and radial gradients with multiple stops.
- [ ] `DropShadow` component rendered with Vello blur.
- [ ] Blending modes: Multiply, Screen, Overlay.

---

# Phase 6: Prepress & Export — NOT STARTED

This phase is what separates a drawing tool from a publishing tool. A layout
that cannot produce a correct PDF/X for a commercial printer is not a DTP
application.

## 1. The Preflight Engine (Live Error Checking)
- [ ] Background preflight system scanning the ECS for print-breaking errors.
- [ ] **Overset text:** flag frames whose layout exceeds their bounds. The
  renderer already computes an `is_overset` flag — preflight can consume it.
- [ ] **Missing/modified links:** path gone, or file hash changed.
- [ ] **Low-resolution images:** effective PPI under a configurable threshold.
- [ ] **Color space mismatches:** RGB assets in a CMYK-destined document.
- [ ] Preflight panel with clickable jumps to the offending entity.

## 2. PDF Generation Engine
- [ ] Integrate `printpdf` or `pdf-writer`.
- [ ] Translation layer: ECS tree to PDF drawing commands.
- [ ] Font embedding with `skrifa` subsets, so a RIP renders it exactly.
- [ ] CMYK conversion on export using the Phase 5 ICC profiles.
- [ ] Target PDF/X-1a and PDF/X-4.

## 3. Print Marks and Bleed
- [ ] `MediaBox`, `TrimBox`, and `BleedBox` setup.
- [ ] Crop marks, bleed marks, registration marks, and color bars.

## 4. Project Packaging
- [ ] "Package Document" command.
- [ ] Collect the project file, `/Links`, and `/Document Fonts` into one folder.
- [ ] Generate a summary: dimensions, fonts, required inks, preflight warnings.

---

## Working Agreement

- Phases are ordered by dependency, not by preference. Phase 6 is what makes
  this software professionally useful; Phases 4 and 5 exist to feed it.
- Every backend change lands with tests. Core tests live inline in a
  `mod tests` block per module; renderer tests that need a GPU live in
  `crates/renderer/tests/`.
- Update this file's checkboxes in the same commit as the work.
