# Tessera Publisher — Rebuild Design

**Date:** 2026-09-01
**Status:** Approved for planning
**Supersedes:** the Tauri + Svelte 5 architecture and Phases 1–6 of the previous ROADMAP

---

## 1. Why this document exists

Tessera Publisher is a professional desktop publishing application — an
InDesign-class layout tool, free, with Linux as a first-class target. The
absence of a serious DTP application on Linux is the reason the project
exists.

Development was halted on 2026-08-31 for a reason worth restating plainly,
because this design is the answer to it:

> Four phases were completed. The application had a dockable workspace, a
> native menu bar, a tools palette, a property inspector, layers and pages
> panels, master pages, text threading, snapping and a preflight engine.
>
> It could not save a document. It could not open one. It had no file
> format, and no phase of the roadmap had ever contained one. It could not
> export a PDF. **Every piece of work a user did vanished when they closed
> the window.**

The roadmap tracked components, not capabilities, and a component-shaped plan
cannot notice that gap — every box was legitimately ticked. This design fixes
the architecture *and* the way progress is measured.

Two things are settled and are not reopened here:

1. **The stack is native Rust with egui.** No Tauri, no webview, no
   TypeScript. One renderer, one input queue, one process.
2. **Persistence and PDF export are the spine.** They ship in the first
   milestone, before anything that looks impressive.

---

## 2. What went wrong architecturally

The previous application composited a native Vello surface *underneath* a
transparent webview. Nearly every serious defect traced to that seam:

- A CSS `height: 100%` that silently resolved to `0` disabled the renderer's
  clip rect. A styling mistake in one language broke rasterization in another.
- Text could not be typed on the canvas. Glyphs were drawn by Vello in Rust;
  keystrokes arrived in the DOM; **nothing shared a text model**, and no
  amount of IPC could invent one.
- "Panels must not be trapped under the canvas" was tracked as a *constraint*.
  It only existed because of the seam.
- Every interaction crossed an IPC boundary, so gesture logic fragmented
  across two languages and two mental models.

No comparable professional application is built this way. Affinity, Blender
and Adobe draw their own interface with their own engine. Figma's canvas sits
*inside* its page, not beneath it. The fix is not a better seam — it is the
removal of the seam.

**The reference implementation is oxiDRAFT** (`crates/oxidraft_*`, 64,711
lines, egui 0.35 + eframe, no webview). Its conventions are lifted
deliberately: the crate graph shape, `theme.rs` design tokens, icons
hand-painted through `egui::Painter`, a `command.rs` layer, and the
`state/` versus `view/` split.

One thing is explicitly **not** lifted: oxiDRAFT renders its canvas with
egui's own tessellator, which is correct for CAD strokes and wrong for DTP.
Tessera needs gradients, blurs, clip groups, transparency groups and glyph
outlines that match what the PDF writer emits. That is Vello's job.

---

## 3. Non-negotiables

| # | Requirement | How it is enforced |
|---|---|---|
| N1 | A document can be saved, closed, and reopened faithfully | Round-trip property tests in `tessera_document`, from milestone 0 |
| N2 | A document can be exported as PDF | Headless export tests in `tessera_pdf`, from milestone 0 |
| N3 | Text can be typed directly on the canvas | Headless editing tests in `tessera_text`, from milestone 0 |
| N4 | The code builds and runs on Linux, Windows and macOS | No platform-specific code outside a single `platform` module; CI matrix |
| N5 | No unsafe code | `unsafe_code = "forbid"` at workspace level |
| N6 | Progress is measured in user capabilities | The roadmap states acceptance criteria as sentences a user could perform |

**On N4:** all three platforms are *built* and CI-tested from the start.
Interactive verification is Windows-only for now, by the author's own choice
and matching how oxiDRAFT is developed. Linux and macOS are verified when the
author has machines to verify them on. What must not happen is
platform-specific code accumulating in the meantime — hence the single
`platform` module rule, which is checkable without a Linux box.

---

## 4. Architecture

### 4.1 Crate graph

Dependencies point downward only. Nothing below `tessera_ui` knows egui
exists; nothing below `tessera_render` knows wgpu exists.

```
apps/
  tessera_app          eframe binary: window, menus, dialogs, autosave, recovery
                       └── depends on: tessera_ui

crates/
  tessera_ui           egui interface: theme, icons, panels, tools, commands,
                       the viewport widget that hosts the rendered document
                       └── document, layout, render, pdf, io, text, color, geometry

  tessera_pdf          PDF/X generation: page tree, content streams, font
                       subsetting, colour conversion, marks and boxes
                       └── layout, document, text, color, geometry

  tessera_render       document to vello::Scene; render to texture, and to pixels
                       └── layout, document, text, color, geometry

  tessera_layout       grids, guides, columns, baseline grid, snapping,
                       text frame threading and reflow
                       └── document, text, geometry

  tessera_document     the document model, undo/redo, AND the .tessera format
                       └── text, color, geometry, io

  tessera_text         font database, shaping, the story model, and the
                       editable buffer with cursor, selection and IME state
                       └── geometry

  tessera_io           file primitives and image decoding. Knows nothing about
                       the document model — see the note below
                       └── color, geometry

  tessera_color        RGB / CMYK / Lab, ICC profiles, swatches, spot inks
                       └── (nothing)

  tessera_geometry     points, rects, affine transforms, beziers, hit-testing
                       └── (nothing)
```

**On `tessera_io`'s position.** An earlier draft had it depending on
`tessera_document` for link resolution and packaging, while `document`'s file
format depended on `io` for atomic writes — a dependency cycle Cargo rejects.
It is resolved by making `io` a *lower* crate than `document`: it owns
filesystem primitives (atomic write, path handling) and image decoding, and
has no knowledge of documents. Resolving a link *against a document*, and
packaging one, are operations on a document and live in `tessera_document`
and `tessera_ui` respectively, calling down into `io` for the file work.

### 4.2 Why each crate exists

Each answers three questions: what it does, how you use it, what it depends on.

**`tessera_geometry`** — a thin, opinionated layer over `kurbo`. Exists so
that "a rectangle in document coordinates" is a distinct type from "a
rectangle in screen pixels", which is the single most common source of bugs
in a zoomable canvas application. Provides `DocPoint` / `DocRect` / `DocAffine`
and their screen counterparts, plus conversion through a `ViewTransform`.
Pure functions, no state, exhaustively unit-tested.

**`tessera_color`** — colour values, ICC profile handling and separation.
Exists at milestone 0 despite the colour engine being a milestone 5 feature,
**for its types rather than its functionality.** If `Fill` is born
RGB-only, adding CMYK later touches every crate, every serialized document,
and every test, and forces a file-format migration. A `Color` enum that can
already hold `Cmyk` and `Spot` variants costs nothing now.

**`tessera_text`** — the hardest crate, and the one whose absence killed the
previous architecture. It owns:

- the font database and family resolution (`fontique`);
- shaping and line breaking (`parley`), producing positioned glyph runs;
- the **story model**: a sequence of styled runs, independent of any frame;
- the **editable buffer**: cursor position, selection anchor, grapheme and
  word boundaries, and the IME pre-edit state.

It has no dependency on egui, on wgpu, or on the document tree. That
isolation is the point: cursor movement over combining characters, selection
across a bidi boundary, and IME composition can all be tested headless, in
hundreds of fast tests, instead of only being exercisable by clicking in a
running window.

**`tessera_document`** — the model and the file format, deliberately in one
crate so they cannot drift. Owns the node arena, the scene graph, styles,
links, and the undo/redo stack. Every type is `Serialize` / `Deserialize`, and
the format module owns versioning and migration.

**`tessera_layout`** — everything that computes *where things go* without
drawing them: margins, columns, ruler guides, the baseline grid, the snapping
solver, and text threading (flowing one story through a chain of frames and
reflowing the chain when any link resizes). Separated from `document` because
it is pure computation over the model, and separated from `render` because a
headless PDF export needs the same answers a screen redraw does.

**`tessera_render`** — translates a document into a `vello::Scene` and
rasterizes it. Two outputs from one scene builder: a wgpu texture for the
live viewport, and a CPU pixel buffer for tests and page thumbnails. The
pixel path is what makes rendering regression-testable without a window.

**`tessera_io`** — filesystem primitives and image decoding: atomic writes,
path handling, decoding raster formats, and downscaled proxy generation and
caching. Deliberately ignorant of the document model, so that
`tessera_document`'s file format can call down into it without a cycle.
Deciding *whether* a link is stale, and collecting links and fonts into a
package, are operations on a document and live above this crate.

**`tessera_pdf`** — generates PDF from the *document*, not from the rendered
scene. See section 8.

**`tessera_ui`** — all egui. Mirrors oxiDRAFT's structure:

```
tessera_ui/src/
  theme.rs        design tokens: colours, spacing, radii, type scale
  icons.rs        icons painted through egui::Painter, no image assets
  command.rs      every user action as a named, undoable command
  state.rs        application state: open document, selection, active tool
  state/          submodules as state.rs grows
  tools.rs        the tool state machine (select, rect, ellipse, text, pen)
  view.rs         top-level frame assembly
  view/
    chrome.rs     menu bar, status bar, docked panel host
    viewport.rs   the document canvas widget — hosts the Vello texture
    panels/       inspector, layers, pages, swatches, links, preflight
    overlays.rs   selection handles, guides, snap indicators, text cursor
```

**`tessera_app`** — the eframe binary. Window creation, wgpu backend
selection per platform, the native menu, `rfd` file dialogs, autosave and
crash recovery. Kept thin: it wires things together and owns nothing.

---

## 5. Decision record

### D1 — The document is an arena of typed nodes, not an ECS

**Decision:** a `slotmap`-backed arena of node structs with explicit
parent/child ID links. Not `bevy_ecs`.

**Rationale.** The previous application used `bevy_ecs`. ECS pays for itself
when systems iterate tens of thousands of entities every frame. A spread
holds dozens of frames, and the render loop is driven by state changes rather
than a 60 Hz tick. The pattern bought nothing and charged for it three times
over: serialization of a `World` is awkward, undo requires snapshotting the
whole `World`, and every read went through a `Mutex<World>` — including reads
from the UI thread during a drag.

An arena of plain structs serializes with serde for free (**the file format
very nearly falls out of the model**), clones cheaply enough that undo can be
snapshot-based, and is trivially unit-testable. oxiDRAFT does exactly this.

**This is the concrete reason "from zero" pays for itself** rather than
merely costing.

**Rejected alternatives.** *Keep ECS and rebuild around it* — pays the
serialization and undo tax for a performance win that never arrives.
*Persistent immutable tree via `im` throughout* — makes undo free, but every
edit path fights the borrow checker and the ergonomics tax is paid on every
feature forever.

### D2 — Vello renders into egui through a wgpu paint callback

**Decision:** eframe with the wgpu backend. The viewport widget allocates a
rect and registers an `egui_wgpu` paint callback. During the callback's
`prepare` phase, Vello renders the document scene into a texture using the
**same `wgpu::Device` and `Queue` that egui already owns**. During `paint`,
that texture is drawn into egui's render pass.

**Rationale.** One device, one frame, one input queue. Panels overlap the
canvas the way any two egui widgets overlap, and the previous architecture's
"panels must not be trapped under the canvas" constraint stops being
expressible. Input arrives through a single `egui::InputState`, so a drag
that starts on the canvas and continues over a panel behaves correctly
without special handling.

**This is the highest-risk decision in the design.** See R1 in section 13.

**Rejected alternatives.** *Render the document with egui's tessellator, as
oxiDRAFT does* — no gradients, no blur, no clip groups, no transparency
groups, and glyph rasterization that would not match PDF output. Correct for
CAD, disqualifying for DTP. *A separate window or child surface for the
canvas* — reproduces the seam that was just removed.

### D3 — One text model, shared by the screen and the PDF

**Decision:** `tessera_text` owns a single story model and a single editable
buffer. On-canvas editing consumes `egui::Event::Text`, `Event::Key` and
`Event::Ime` and mutates that model directly. Parley reshapes. The resulting
**positioned glyph runs are consumed by both `tessera_render` and
`tessera_pdf`.**

**Rationale.** Sharing the shaped output is what guarantees the export
matches the screen. It is also the capability the previous architecture made
structurally impossible: glyphs drawn in Rust, keystrokes delivered to the
DOM, and no shared model between them.

**Note on immediate-mode UI.** Text entry is the known hard part of
immediate-mode interfaces, because the widget holding the cursor is
reconstructed every frame. The resolution is that the cursor and selection
live in `tessera_text`'s buffer — which is persistent application state —
and the egui layer only *reports events into it*. egui's own `TextEdit`
state is not used for canvas text.

### D4 — `.tessera` is a zip container, and it ships in milestone 0

**Decision:** a `.tessera` file is a zip archive:

```
document.json          the serialized document model
meta.json              format version, application version, created/modified
thumbnail.png          first spread, for file browsers and the start screen
links.json             manifest of linked assets: path, hash, size, mtime
fonts/                 embedded fonts, only when the user packages the document
links/                 embedded assets, only when the user packages the document
```

**Rationale.** A container makes packaging (collect links and fonts into one
deliverable) the *same mechanism* as saving, rather than a separate feature
built later. It permits a thumbnail without parsing the document. It allows
recovery tooling to read `document.json` directly out of a damaged file. It
is what `.idml`, `.sketch` and `.docx` all do, for these reasons.

`meta.json` carries a monotonically increasing `format_version`. Loading a
document whose version is older runs migrations in sequence; loading one that
is newer refuses with a clear message rather than corrupting it.

**Rejected alternatives.** *A single JSON file* — cannot carry a thumbnail or
embedded assets, so packaging becomes an unrelated second mechanism. *A
binary format* — premature; JSON is diffable, greppable and debuggable, which
matters far more during development than the bytes it costs. Revisit only if
profiling shows load time is a real problem.

### D5 — Undo is snapshot-based

**Decision:** the undo stack holds document snapshots, bounded by count and
by total memory. Commands mutate the document; the command layer pushes a
snapshot before mutating.

**Rationale.** D1 makes the document cheap to clone. Snapshot undo cannot
develop the class of bug where an inverse operation is subtly wrong — which
the previous implementation had, with add and remove page never being
undoable at all because no inverse was ever written. If profiling later shows
large documents make this unacceptable, individual hot commands can be given
explicit inverses without changing the interface.

---

## 6. The document model

```
Document
  ├─ meta            title, author, created, modified
  ├─ settings        page size, facing pages, bleed, slug, DPI, colour intent
  ├─ styles          paragraph styles, character styles, object styles
  ├─ swatches        named colours, including spot inks
  ├─ stories         text stories, addressed by StoryId, independent of frames
  ├─ masters         master spreads, same node structure as spreads
  └─ spreads         ordered list of spreads
       └─ pages      one or two per spread
            └─ layers
                 └─ frames    the leaves of the node arena
```

A `Frame` holds a transform, a size, a `FrameKind`, and a style reference:

```
FrameKind::Rectangle
FrameKind::Ellipse
FrameKind::Path(BezPath)
FrameKind::Text { story: StoryId, thread_next: Option<FrameId> }
FrameKind::Image { link: LinkId, fit: FitMode, inner_transform: Affine }
FrameKind::Group(Vec<FrameId>)
```

**Stories are addressed separately from frames.** A threaded story flows
through several frames but exists once. This is the structure that makes
text threading natural rather than bolted on, and it is why `stories` sits at
the document level rather than inside a frame.

---

## 7. Rendering pipeline

```
Document ──► tessera_layout ──► resolved geometry, guides, shaped text
                    │
                    ▼
             tessera_render ──► vello::Scene
                    │
        ┌───────────┴────────────┐
        ▼                        ▼
  render_to_texture        render_to_pixels
  (wgpu texture,           (CPU buffer, headless —
   drawn by egui)           tests and thumbnails)
```

Redraws are triggered by a document revision counter, not by a timer. A
camera pan changes the view transform without touching the document, so it
re-renders the scene but does not rebuild it.

Overlays — selection handles, snap indicators, ruler guides, the text
cursor, text thread connectors — are drawn by **egui's painter, on top of the
Vello texture**, not by Vello. They are interface, not document: they must
not appear in an export, they need no gradient support, and drawing them in
egui means they can respond to hover in the same frame they are drawn.

---

## 8. PDF export

**`tessera_pdf` depends on `tessera_document`, and never on
`tessera_render`.** The PDF is generated from the document model directly.

**Rationale.** Vello is a screen rasterizer; a PDF is a vector program. An
application that generates its export from its screen scene ends up with
"the export doesn't match the screen" defects that cannot be fixed, because
the two pipelines have diverged by construction. Here, both consume the same
shaped glyph runs from `tessera_text` and the same resolved geometry from
`tessera_layout` — the shared source is what makes them agree.

Pipeline:

1. `pdf-writer` builds the page tree, resources and content streams.
2. Fonts are subsetted and embedded — the subsetting crate is selected during
   milestone 0; `skrifa` reads glyph data but does not itself subset.
3. Colours convert through `tessera_color` using the document's output
   intent, via `lcms2`.
4. `MediaBox`, `TrimBox` and `BleedBox` come from document settings; marks
   are drawn into the content stream outside the trim box.
5. Targets are PDF/X-1a (CMYK, flattened) and PDF/X-4 (live transparency,
   ICC-tagged).

Milestone 0 ships step 1 with embedded fonts and RGB colour — a valid PDF
that opens in Acrobat. Steps 3 through 5 arrive in milestone 6.

---

## 9. Error handling

Three tiers, deliberately distinguished:

- **User errors** — a missing linked image, an unsupported font, a document
  from a newer version. Surfaced in the interface with a stated cause and an
  offered action. Never a panic, never a silent fallback.
- **Recoverable faults** — a GPU adapter that fails to acquire, a save that
  fails on a full disk. The application stays running, states what failed, and
  keeps the document in memory. **A failed render must never lose a document**;
  this is why preflight, save and export are independent of the GPU.
- **Bugs** — invariant violations. `debug_assert!` in development; in release
  the application writes the in-memory document to a recovery file before
  terminating.

Autosave writes a recovery copy on a timer and after every N commands. On
launch, an unclaimed recovery file offers restoration.

**Silent fallbacks are forbidden.** The single most instructive defect in the
previous codebase was a clip rectangle that resolved to zero and disabled
rendering without a word.

---

## 10. Testing strategy

| Layer | Approach | Runs where |
|---|---|---|
| `geometry`, `color`, `text`, `layout` | Unit tests inline, plus `proptest` for transforms, cursor movement and reflow invariants | Every commit, fast |
| `document` | **Round-trip property tests**: arbitrary document, save, load, assert identical. Plus migration tests holding fixture files from every past format version | Every commit, fast |
| `pdf` | Export fixture documents, parse the output back, assert structure. Byte-compare against approved reference files | Every commit, fast |
| `render` | `render_to_pixels` against approved reference images, with a tolerance | Every commit, no window needed |
| `ui` | `egui::Context` driven headlessly: inject events, assert state transitions | Every commit |
| Integration | The full milestone-0 sentence, driven headlessly end to end | Every commit |
| GPU | Live adapter tests | **Manually, alone, never in `--workspace`** — see the known intermittent adapter-acquisition hang |

The property test on save and load round-tripping is the most valuable single
test in the suite. It is what makes N1 a structural guarantee rather than a
hope.

---

## 11. Interface design

The look follows oxiDRAFT's approach: a `theme.rs` module of design tokens —
colour roles, spacing scale, corner radii, type scale — with **no literal
colours or magic numbers anywhere else in the UI crate**. Icons are painted
through `egui::Painter` rather than loaded as assets, so they scale crisply
at any DPI and re-tint with the theme.

This is recorded honestly: **egui gives nothing for free aesthetically.**
Every widget a DTP application needs beyond a button — rulers, colour
pickers, swatch grids, gradient editors, dockable panel stacks, modal
dialogs, numeric scrub fields — has to be built. oxiDRAFT proves this is
achievable and does not look like a debug tool. The cost is real and is
planned for in the roadmap rather than discovered during it.

**Docking is deliberately late** (milestone 7). Building the workspace chrome
before the application could keep a user's work is precisely the mistake this
plan exists to correct. Until then the layout is fixed: a tool strip, one
inspector, and the canvas.

---

## 12. Platform strategy

- Platform-specific code lives **only** in `tessera_app/src/platform/`. This
  rule is checkable on Windows, so it holds even while Linux is unverified.
- The wgpu backend is chosen per platform: Vulkan on Linux, DX12 on Windows,
  Metal on macOS, each with a documented fallback.
- CI builds and runs the headless test suite on all three from milestone 0.
  A compile break on Linux is caught the day it lands.
- Interactive verification is Windows-only until the author has other
  machines. Wayland fractional scaling, IME behaviour and native dialogs are
  logged as **known-unverified**, not as done.
- Packaging — AppImage or Flatpak, MSI, DMG — is milestone 8.

---

## 13. Risks

**R1 — wgpu version alignment between Vello and eframe.** D2 requires Vello
and `egui-wgpu` to agree on a `wgpu` version, since they share a device.
Independent release cycles make this a real hazard. **This is verified first,
in the opening step of milestone 0, before anything else is built.** If the
versions cannot be reconciled: fall back to rendering Vello on its own device
into a CPU buffer and uploading it as an egui texture — correct, portable,
and slower; or pin both crates and carry a patch. Either fallback is
survivable, but the answer must be known on day one, because a negative
result changes the design.

**R2 — text editing and IME in immediate mode.** Mitigated by D3, and by
studying oxiDRAFT's existing handling first. The remaining unknown is IME on
Linux, which cannot be verified yet and is logged as such.

**R3 — the widget-building cost.** Named in section 11 rather than
discovered. Milestone 7 is sized for it, and nothing before milestone 7
depends on polished chrome.

**R4 — greenfield loses proven integration work.** The previous
`crates/renderer` had a working Vello, parley and preflight integration.

**This risk is accepted in full, not mitigated.** The rebuild is **clean-room
by the author's explicit decision (2026-09-01): nothing is reused.** The old
code is not read, not ported, and not consulted — not from the working tree,
not from git history, not from the GitHub remote. Every crate is written from
upstream documentation and from the spike note in
`docs/superpowers/notes/`.

The cost is real and is paid deliberately: device setup, row-stride handling
on texture readback, parley integration and image proxy decoding all get
solved a second time. What is bought is a codebase with no inherited
assumptions from an architecture that failed, and no half-ported code whose
original context no longer exists.

**R5 — `lcms2` introduces a C dependency**, which complicates
cross-compilation and packaging. Accepted: ICC v4 and CMYK separation are
non-negotiable for PDF/X, and the pure-Rust alternative (`qcms`) does not
cover them. Confirmed to build on all three platforms during milestone 5, not
later.

---

## 14. Explicitly out of scope

Removed by YAGNI, and recorded so they are not quietly reintroduced:

- Floating (undocked) panels. Docked stacks and an icon rail only.
- Plugin or scripting APIs.
- Collaborative or multi-user editing.
- Cloud storage and sync.
- A binary document format (see D4).
- Any format import — IDML, PDF, SVG — before milestone 8. Reading other
  applications' formats is a project of its own.
- Tabbed documents. One document per window until milestone 7.

---

## 15. Milestone 0 — the definition of done

Milestone 0 is complete when a person can perform this sentence, on Windows,
against a release build, with no developer tooling:

> Launch Tessera. A new document opens with one spread. Draw a rectangle and
> give it a fill colour. Draw a text frame, **type into it on the canvas**,
> and see the text shaped and rendered. Save the file as `.tessera`. Quit the
> application. Launch it again, open that file, and find the rectangle and
> the text exactly as they were left. Export a PDF, and open that PDF in
> Acrobat with the text selectable.

Every crate in section 4.1 exists and is real. None is deep. No docking, no
master pages, no threading, no swatch panel, no preflight.

**When that sentence works, the application is worth building on. Until it
does, nothing else counts.**
