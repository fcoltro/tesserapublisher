# Tessera Publisher — Roadmap

A professional desktop publishing application: an InDesign-class layout tool,
free and genuinely cross-platform. Linux is a first-class target, not an
afterthought — the absence of a serious DTP application on Linux is the reason
this project exists.

**Architecture:** native Rust, egui 0.35 + eframe, Vello for the document
surface. No webview. See
[the rebuild design](docs/superpowers/specs/2026-09-01-tessera-rebuild-design.md).

**Interface:** InDesign-informed, not InDesign-shaped. The element-by-element
comparison is [`docs/INDESIGN-PARITY.md`](docs/INDESIGN-PARITY.md); the
direction is
[the Instrument spec](docs/superpowers/specs/2026-09-03-instrument-milestone-design.md).

---

## How to read this file

**A milestone is done when a person can perform its sentences.** Not when its
components exist.

This rule is the correction to a specific failure. The previous roadmap
tracked components, and by its own accounting four phases were complete: a
dockable workspace, native menus, a property inspector, master pages, text
threading, snapping, preflight. Every box was legitimately ticked. **The
application could not save a document, open one, or export a PDF** — there was
no file format anywhere in the codebase *or* in the plan. A component-shaped
roadmap cannot see a gap like that, because nothing was ever unticked.

So every milestone below states its acceptance criteria as **capabilities** —
sentences a person could carry out — and lists components only as the means.

**Status legend:** `[x]` verified by performing the sentence · `[~]` partially
true, with the shortfall stated · `[ ]` not started

> Before marking anything `[x]`, perform the sentence. Not the test suite —
> the sentence.

---

## Cross-cutting requirements

Never "done" until shipping; re-checked at the close of every milestone.

- [~] **Save and open never lose data.** Round-trip property tests pass on
  arbitrary generated documents, and the migration chain is now exercised:
  format version 2 added frame rotation, and a hand-built version 1 archive is
  loaded by a test to prove an older document still opens.
- [x] **Cross-platform build parity.** CI builds and runs the 129-test
  headless suite on Linux, Windows and macOS — **all three green as of
  2026-09-01**. `apps/tessera_app/src/platform/` is empty, so no
  platform-specific code exists yet. This is the project's defining
  requirement and it is now checked on every push.
- [~] **Interactive verification.** Windows only, by choice. Linux and macOS
  are **known-unverified** and are recorded as such, never as done.
- [x] **No unsafe code.** `unsafe_code = "forbid"` at the workspace level.
- [~] **No silent fallbacks.** Holds across the code written so far: every
  error path returns a stated cause, and file failures reach the status bar.
  Re-checked each milestone rather than assumed. The previous codebase's most instructive defect was a clip
  rectangle that resolved to zero and disabled rendering without a word.
- [x] **Tests land with the change**, in the same commit.
- [x] **Every mutation is covered by exactly one undo entry.** Changes go
  through `Command`. The exception is an interactive gesture, which must write
  live or nothing would be visible until the mouse came up, and which is
  legitimate only because it brackets those writes — one snapshot when it
  begins, or a restore and a single `Command` when it ends. A direct mutation
  outside the command layer must therefore say so with an `undo-bracketed:`
  marker, and `tests/command_invariant.rs` fails on any that does not.
  *The first draft of this rule read simply "every mutation goes through
  `Command`". Asserting it turned up seven direct mutations: two test fixtures
  and five deliberate, commented, bracketed gestures. The rule was wrong, not
  the code — which is the argument for asserting a rule rather than stating
  one.*
- [ ] **The application is operable from the keyboard alone**, and every
  control carries an accessible name through egui's AccessKit support.
  Retrofitting this costs many times what designing for it does, and a
  publishing tool that a screen reader cannot describe is not finished.
  Re-checked at the close of every milestone.
- [x] **Performance is measured, not asserted.** A guard over a 500-frame
  document holds resolve and scene-build time under one whole frame. Baseline
  on the development machine, 2026-09-03: **0.41 ms**, roughly fifty times
  under the ceiling. A number nothing measures is a wish.

---

# Milestone 0 — The Walking Skeleton

**The spine. Nothing else counts until this works.**

> **COMPLETE — 2026-09-01.** The sentence below was performed by hand, in the
> running application, on Windows. Not the test suite: the sentence.
>
> 139 tests pass alongside it (134 headless + 5 GPU-backed), `clippy -D
> warnings` is clean, and the sentence also runs end to end headlessly in
> `crates/tessera_ui/tests/milestone_0.rs`.
>
> **Tessera can now keep a user's work.** That is the whole point of this
> milestone and the correction to what came before.
>
> Verified interactively on Windows only. Linux and macOS **build and pass
> the headless suite in CI** but have never been run as an application —
> known-unverified, not done.

Every crate exists and is real. None is deep. This milestone deliberately
produces an application that does very little and *keeps every bit of it*.

### Acceptance — one sentence, performed on Windows against a release build

> Launch Tessera. A new document opens with one spread. Draw a rectangle and
> give it a fill colour. Draw a text frame, **type into it on the canvas**,
> and see the text shaped and rendered. Save the file as `.tessera`. Quit the
> application. Launch it again, open that file, and find the rectangle and the
> text exactly as they were left. Export a PDF, and open that PDF in Acrobat
> with the text selectable.

- [x] **Step 0 — the wgpu spike.** Prove Vello can render into a texture on
  the device eframe owns, on egui 0.35. **This runs before anything else is
  built**, because a negative result changes the design (see R1 in the spec).
- [x] **Step 1 — demolition.** Remove `src/`, `src-tauri/`, `crates/core`,
  `crates/renderer`, and all Node tooling. The old tree stays in git history
  and is consulted, not carried.
- [x] **Step 2 — the workspace.** Nine crates and one app, dependencies
  pointing downward only, `unsafe_code = "forbid"`, CI on three platforms.
- [x] A window opens, egui draws, the canvas pans and zooms.
- [x] Rectangles and text frames can be drawn, selected and moved.
- [x] Text is typed **on the canvas**, with a caret, selection, and backspace.
  The capability the previous architecture made structurally impossible.
- [x] `.tessera` saves and loads, with round-trip property tests.
- [x] PDF exports with embedded, subsetted fonts and RGB colour.
- [x] Undo and redo work across every one of the above.

**Explicitly not in M0:** docking, panels beyond one inspector, master pages,
threading, swatches, preflight, images, CMYK, print marks.

---

# Milestone 1 — The Editing Surface

Making the skeleton pleasant to use. No new file-format surface area.

> **Status 2026-09-03: the original scope is code complete, awaiting the
> manual check; five items were added afterwards and are not built.**
> Everything down to the undo-entry line is built and tested. As with
> milestone 0, the boxes stay unticked until a person performs the sentence in
> the running application. The five items below that line came out of the
> InDesign reading on 2026-09-03 — the reference point, shear, align and
> distribute, corner options, and the remaining tools — and no code exists for
> any of them.
>
> A first run on 2026-09-02 produced a punch list, worked through in
> `docs/UX-PASS-1.md`: painted Lucide cursors, transform zones that do not
> overlap, an authoritative group box, shape-precise selection and marquee,
> on-canvas text editing with a real caret, and two antialiasing fixes. That
> pass has not itself been looked at yet.

### Acceptance

> Draw rectangles, ellipses, lines and free paths. Select several objects at
> once with a marquee and move them together. Rotate an object and scale it
> from any handle. Nudge with arrow keys. Copy, paste, duplicate and delete.
> Zoom to fit, zoom to selection, and pan with the spacebar. Undo any of it,
> then redo it.

- [~] Tool state machine: select, rectangle, ellipse, line, pen, text, hand.
  *Direct-select and zoom tools are not built; the wheel zooms instead.*
- [x] Marquee selection, shift-extend, and select-all. Both clicking and the
  rubber band select by an object's geometry, not by its bounding box.
- [~] Transform handles: move, scale from any of eight handles, rotate by
  dragging outside a corner, with shift for proportional scaling and
  15-degree rotation snap. A group scales and rotates as one, carrying its
  contents. *From-centre scaling is not built, and a multiple selection has
  no handles — one frame or one group at a time.*
- [~] Clipboard and duplicate. *Step-and-repeat is not built.*
- [x] Z-order: bring forward, send backward, to front, to back — correct for
  multiple selections, which needs opposite traversal orders per operation.
- [x] Grouping and ungrouping, including nested groups.
- [x] Numeric transform fields with drag-to-scrub, including rotation.
- [x] Every gesture records exactly one undo entry, on completion.
- [ ] **Reference point**: transforms resolve about a chosen one of nine
  anchors, which subsumes the from-centre scaling missing above.
- [ ] **Shear**, with an honest affine decomposition replacing
  `Transform::rotation_degrees()`'s assumption that no shear exists.
- [ ] **Align and distribute** across a multiple selection.
- [ ] Corner options and corner radius. *(Model change: a format version
  bump.)*
- [ ] Direct-select and zoom tools; add, delete and convert anchor points;
  polygon; scissors.

---

# Milestone 1.5 — The Foundation and the Instrument

**Three ordered phases. The invisible things first, the page second, the
interface last.**

> Prompted by reading an InDesign screenshot against the codebase on
> 2026-09-03. Design:
> [the Instrument spec](docs/superpowers/specs/2026-09-03-instrument-milestone-design.md).

`Stroke` carries alignment, caps, joins, miter limit, dashes and dash offset —
built, tested, and exposed nowhere. `Color` models CMYK and spot; nothing can
create either. The inspector renders position, size, rotation, text and fill,
and stops.

This does not break the rule that defers the workspace to milestone 7. That
rule forbids chrome *ahead of* capability. This is the surface for capability
already finished.

**The phases are strictly ordered.** Phase A builds the things that have no
UI and no file-format surface, and that half of everything below depends on.
Phase B makes the page a real page — the single format version bump of this
milestone, batched deliberately because each bump costs a migration test and
five scattered bumps cost five. Phase C is the interface, which cannot be
built well before either.

Every task lands with its tests in one commit. A task that cannot is too big
and gets split.

### Acceptance

> Select a rectangle. Set its position and size numerically with the reference
> point on its centre, and watch it scale about that point — with the anchor
> mark visible on the object as it does. Type `12mm` into a field reading
> points and see it convert. Give the rectangle a 3 pt dashed stroke, aligned
> inside, with round caps, and see it on the canvas. Swap fill and stroke with
> one key. Select three objects and align their left edges from the toolbar
> beside them. Shear one. Press `Ctrl`+`K`, type "flip", and flip it. Read its
> position off a ruler in millimetres, switch the ruler to picas, and watch
> every field in the application follow. Drag a guide off that ruler and align
> the object to the page margin. Give the document a 3 mm bleed and see it
> drawn. Press `W` and see the handles, frame edges, guides and rulers go,
> leaving the page on a neutral surround; press it again and get them back.
> Switch to the light theme and read every label. Save, quit, reopen, and find
> the page setup and the guide exactly as they were left. Force-quit instead,
> relaunch, and be offered the recovered document.

## Phase A — Foundations

Nothing here appears on screen and nothing here touches the file format. Each
task is a small, pure, independently testable piece that later work stands on.
Built first because retrofitting any of them is many times the cost.

> **Status 2026-09-04: nine of ten done, one partial. Code complete.**
>
> A phase-A item has no sentence a person can perform — that is what makes it
> phase A. So `[x]` here means the narrower thing: the code exists, its tests
> pass, and nothing about it is visible to check by hand. Where a visual check
> *is* owed, the item stays `[~]` and says so. This is a deliberate reading of
> the legend above, not an exemption from it.
>
> **Two hand checks are owed and neither has been done.** A5 restructured
> `TesseraApp` — the milestone-0 spine — across roughly 290 call sites, so
> milestone 0's sentence needs performing again; the headless
> `milestone_0.rs` passes unchanged, which is evidence and not proof. A9
> changed how icons are built, and nobody has looked at the tool strip since.
> Until both are done, phase A is code-complete rather than complete.
>
> Every task is built. Phase B is next, and it is planned separately.

- [x] **A1 — Units.** A `Unit` type over mm, pt, px, inches and picas, with
  parsing (`12mm`, `1p6`, `.5in`), formatting and conversion. Property-tested
  round-trips. Every numeric field in the application depends on it.
- [x] **A2 — Preferences store.** A versioned struct written through
  `tessera_io::write_atomic` to the platform config directory, defaulting
  cleanly when absent and **reporting** — never swallowing — a corrupt one.
  First consumers: the preferred unit and the theme.
- [x] **A3 — Affine decomposition.** `Transform::decompose()` into scale,
  shear, rotation and translation, with a recompose property test. Callers of
  `rotation_degrees()` migrate off its no-shear assumption one at a time.
  Unblocks shear, scale-as-percentage and the reference point.
- [x] **A4 — Anchor resolution.** The nine-point anchor as a type, and the
  resolution of scale, rotation and flip about it. Pure geometry, no UI.
- [x] **A5 — The open-document container.** `TesseraApp` held `document`,
  `history`, `resolved`, `view` and `selection` as flat fields; all five are
  per-document. They move into an `OpenDocument`, with the application holding
  a map and an active id. **One document is still open at a time** — the tabs
  are milestone 7. Done now because this refactor widens with every milestone.
- [x] **A6 — The command invariant.** Every mutation routes through the
  `Command` enum, or is a bracketed interactive gesture carrying an
  `undo-bracketed:` marker that says why. Asserted by
  `tests/command_invariant.rs`, which reads the crate's own source because
  Rust can restrict a method to a crate but not to one sibling module.
- [x] **A7 — Performance harness.** A benchmark that builds a 500-frame
  document and measures resolve and scene build, with a regression assertion.
  *The 16.7 ms budget in the spec is a wish until something measures it.*
- [x] **A8 — Theme tokens, light and dark**, with a test asserting WCAG AA
  contrast for every foreground-on-background pair rather than checking by eye.
- [~] **A9 — Icon cache.** Lucide paths parse to `BezPath` once and are cached
  by `Icon`, instead of being re-parsed on every paint. *Tests cover the
  geometry and the cache; the tool strip has not been looked at since the
  change, so the pixels are unverified.*
- [x] **A10 — Autosave and crash recovery.** A periodic atomic write to a
  recovery path, detected and offered on the next launch. Data safety belongs
  with the cross-cutting rules, not at milestone 7.

## Phase B — The Page

> **Status 2026-09-04: code complete. Seven of eight built; B4 dropped.**
>
> 394 tests pass and clippy is clean at `-D warnings`. The format moved from
> 4 to 5 exactly once, and a version-4 document is proven to still open.
>
> **The sentence below has not been performed by hand.** Until it has, this is
> code complete rather than complete.

**One format version bump, one migration test, all of the page geometry at
once.** Moved here out of milestone 3 because rulers, screen modes,
align-to-page, `TrimBox` and `BleedBox`, and preflight's out-of-bleed rule all
stand on it — and because PDF export already ships without a bleed box.

- [x] **B1 — Page geometry.** Size, margins, bleed and slug, with named
  presets (A3/A4/A5/Letter/Legal/Tabloid) and an orientation that turns a page
  without losing its paper.
- [x] **B2 — Facing pages.** The flag, and correct left/right spread geometry.
- [x] **B3 — Guides as document data.** A guide is an axis, a position and a
  spread — spread-level only; page-level guides differ only once pages within
  a spread move independently, which is milestone 3's concern. Landing here rather than at milestone 4 costs nothing extra, because
  the format bump is already being paid — and it is what lets phase C's rulers
  actually yield a guide.
- [ ] ~~**B4 — `ColorRef::{ Direct, Swatch }`.**~~ **Dropped 2026-09-04, to
  milestone 5 where it belongs.** The argument for reserving it early was that
  adding the indirection at milestone 5 would mean migrating every fill and
  stroke in every saved document. Reading `format/mod.rs` undermines that:
  `rotation_to_transform` already does exactly this kind of mechanical JSON
  rewrite in about twenty lines, and wrapping every colour in `Direct` is the
  same shape of walk. The cost is therefore roughly equal now and later, the
  benefit before milestone 5 is nil, and reserving a shape before swatch
  semantics are designed risks reserving the wrong one. YAGNI.
- [x] **B5 — A spread renders as a spread.** `build_scene` takes one page
  today. Margins, bleed and slug are drawn.
- [x] **B6 — Format version 5**, with a migration test proving a version-4
  document still opens and gains no setup it never had. *Not version 3: the
  format is already at 4 — 2 added frame rotation, 3 replaced it with a full
  affine transform, 4 added stroke alignment, caps, joins and dashes. The
  earlier entry here was written from a stale reading and is corrected.*
- [x] **B7 — Document setup inspector**, the "no selection" state: preset,
  size, orientation, facing pages, margins, bleed and slug, each in the
  preferred unit.
- [x] **B8 — `TrimBox` and `BleedBox` in the PDF.** Exporting a document that
  has a bleed and not recording it discards the user's intent silently, which
  the cross-cutting rules forbid. PDF/X proper remains milestone 6.

## Phase C — The Instrument

The interface, built last, on foundations that now exist.

Split into three plans, because these are three subsystems that each produce
working software alone: **C-i the rail** (C1–C5), **C-ii the surface**
(C6–C9), **C-iii the chrome** (C10–C13).

> **Status 2026-09-04: C-i and C-ii are code complete. C-iii not started.**
>
> 462 tests pass and clippy is clean at `-D warnings`.
>
> **Two bugs in this work were found by using the application, not by the
> suite** — page setup not invalidating the resolve cache, and autosave
> failing on a directory that had never been created. Both now have
> regression tests at the level the failure was really at. That is the
> argument for the hand checks, made concrete.
>
> C-ii's own sentence has not been performed by hand.

- [x] **C1 — Inspector shell** with a stable section order — Transform, Fill,
  Stroke, Text, Frame. The *order* is what keeps a hidden section from moving
  anything: universal sections first, so only Text and Frame can be absent and
  they come last. *(D1's original wording — hiding moves nothing — was not
  implementable, and the spec is amended.)*
- [x] **C2 — Reference-point proxy**, with the chosen anchor also drawn on the
  selection itself (A4). Settling `Anchor::shear`'s sign found that phase A's
  decomposition never pinned which way a positive shear leaned; it does now.
- [x] **C3 — Numeric fields**: every field parses a unit suffix (A1) — typing
  `12mm` into a field showing points converts it — with a
  constrain-proportions chain, scale as a percentage, rotation and shear, all
  read from one `Decomposition` and written back as deltas about the
  reference point (A3).
- [x] **C4 — Stroke section**: weight, colour, alignment, cap, join, miter
  limit and dash presets — the shipped model, reachable at last. The miter
  limit and the dash offset appear only when they mean something.
- [x] **C5 — Fill and stroke proxy**, with swap, defaults and none, on `X`,
  `D` and `/` — bound below the text-editing guard so typing never triggers
  them.
- [~] **C6 — Align and distribute.** All four targets exist in the model —
  selection, margins, page, spread — and each is tested. *Only "to the
  selection" is reachable from the interface; the other three wait on a
  target picker.*
- [~] **C7 — Canvas toolbar**: six aligns and two distributes, beside the
  object, appearing only for two or more. Placement is tested against the
  window's edges. *Flip and rotate 90° are not on it, and the buttons carry
  short text labels rather than Lucide glyphs — those come with C13.*
- [~] **C8 — Rulers**, with a 1-2-5 tick ladder, a unit selector that saves
  the preference, and a guide you can drag off either one — dropped back on a
  ruler, the drag is cancelled (B3). *The zero point is fixed at the first
  page's top-left; it is not yet a widget you can drag. An existing guide
  cannot be moved or deleted once placed — the commands exist and are tested,
  the canvas interaction does not.*
- [~] **C9 — Screen modes.** All four exist and are tested, and `W` puts the
  interface away and brings it back: handles, frame edges, margin and bleed
  rules, rulers, guides and the canvas toolbar all go, and the surround
  becomes the fixed neutral grey of D8. *`W` toggles Normal and Preview only —
  Bleed and Slug are not reachable without a View menu (C12) — and the
  document is not yet clipped to the revealed rectangle, so Preview hides the
  furniture without cropping the pasteboard.*
- [ ] **C10 — Status bar**: zoom control and page navigator.
- [ ] **C11 — Command palette** over the `Command` enum (A6), showing each
  command's shortcut beside it.
- [ ] **C12 — Layout, Type, View and Window menus**, carrying only commands
  that exist.
- [ ] **C13 — Icon set** grown to roughly 60 Lucide glyphs (A9).

**Explicitly not in M1.5:** corner radius, opacity, effects, object styles,
gradients, the swatches panel, text wrap, frame fitting, text-frame options,
image links, parent pages, the pages panel, the layers panel, snapping,
dockable panels, document tabs.

---

# Milestone 2 — Typography

The reason a layout tool is not a drawing tool.

### Acceptance

> Set a paragraph in a chosen family, weight, size and leading. Adjust
> tracking and kerning. Set alignment and justification, indents, and space
> before and after. Define a paragraph style, apply it to several paragraphs,
> change the style, and watch every one of them update. Type in a language
> that needs an IME and see the composition preview on the canvas.

- [ ] Font enumeration and family/style resolution across all three platforms.
- [ ] Character formatting: family, weight, style, size, leading, tracking,
  kerning, case, baseline shift.
- [ ] Paragraph formatting: alignment, justification, indents, space before
  and after, hyphenation, drop caps.
- [ ] Paragraph and character styles, with live cascade on edit.
- [ ] Text selection by click-drag, double-click word, triple-click paragraph.
- [ ] IME composition rendered on canvas — **verified on Windows; Linux and
  macOS recorded as unverified.**
- [ ] Right-to-left and bidirectional text render correctly.
- [ ] Typography inspector panel.

---

# Milestone 3 — Document Structure

### Acceptance

> Add, delete, duplicate and reorder pages, and undo any of it. Work in
> facing-page spreads. Put repeating elements on a master page, apply it to
> many pages, and override one instance locally without breaking the others.
> Organise objects onto named layers, then hide and lock a layer.

- [ ] Pages panel: a visual grid of spreads, with drag-to-reorder.
- [ ] Add, delete and duplicate pages — **all undoable.** (The previous
  implementation never made add or remove page undoable, because no inverse
  was ever written. D5's snapshot undo removes that failure mode.)
- [ ] Facing-page spreads with correct left/right geometry. → **moved to
  milestone 1.5, phase B**, along with page size, margins, bleed, slug, and a
  spread that renders as a spread. Tracked there, not here.
- [ ] Master pages, applied by drag, rendered behind page content.
- [ ] Master item override, promoting one item to a local editable copy.
- [ ] Layers panel: named layers, reorder, visibility, lock.
- [ ] Document setup: page size, orientation, margins, bleed, slug. → **moved
  to milestone 1.5, phase B.** Too much stands on it to leave it this late:
  rulers, screen modes, align-to-page, `TrimBox` and `BleedBox`, and
  preflight's out-of-bleed rule. Per-page size overrides stay here.
- [ ] Screen modes Bleed and Slug → **moved to milestone 1.5, phase C**, since
  phase B supplies the geometry they need.

---

# Milestone 4 — Layout Systems

### Acceptance

> Drag ruler guides and have objects snap to them, to the page edge, to the
> margins, and to each other. Set up a multi-column text frame. Turn on a
> baseline grid and lock text to it. Thread a long story through three frames
> across two pages, resize the first, and watch the text reflow through the
> chain. See the connector lines between linked frames when one is selected.

- [ ] Rulers with unit selection (mm, pt, px, in, picas) → **moved to
  milestone 1.5**: the unit type to phase A, the ruler to phase C.
- [ ] Ruler guides → **moved to milestone 1.5**: the data to phase B, the
  drag-out to phase C. They rode along free on a format bump that was being
  paid anyway. Margin guides are drawn by phase B; **column guides stay here**,
  because columns are text-frame geometry that phase B does not model.
- [ ] Snapping solver with a pixel-threshold lock and visible indicators.
- [ ] Baseline grid with a per-frame lock toggle.
- [ ] Multi-column text frames with gutter control, **frame inset, and
  vertical justification**.
- [ ] **Text threading**: overflow flows to the next frame, and a resize
  reflows the whole chain.
- [ ] **Thread connector lines drawn on selection.** (The previous
  implementation had a working story model and never drew these — the
  capability was invisible to the user.)
- [ ] Text wrap around objects.

---

# Milestone 5 — Colour and Assets

### Acceptance

> Place a photograph, move and scale it inside its frame, and fit it to the
> frame proportionally. See its effective PPI and get a warning below 300.
> Replace the file on disk and watch the link update. Define a CMYK swatch and
> a spot colour, apply them, edit a swatch, and see every object using it
> change. Assign a document ICC profile and see a soft proof on screen. Fill a
> shape with a gradient and give it a drop shadow.

- [ ] Image placement with linked (never embedded) assets.
- [ ] Content-within-frame: independent inner transform, fit and fill modes.
- [ ] Clipping of raster content by its container shape.
- [ ] Link status: OK, missing, **and modified** — with relink and update.
- [ ] Effective-PPI reporting with a configurable warning threshold.
- [ ] Disk-backed proxy cache, so downscaling survives a restart.
- [ ] `lcms2` integration, **confirmed building on all three platforms.**
- [ ] RGB, CMYK, Lab and spot colour throughout the model.
- [ ] Swatches panel with global colours that cascade on edit.
- [ ] Document output intent with on-screen soft proofing.
- [ ] Linear and radial gradients; drop shadow; multiply, screen and overlay
  blending.
- [ ] **Object opacity** as a field distinct from blend mode, and distinct
  again from a fill colour's alpha.
- [ ] **A graphic frame is not a shape.** The placeholder frame InDesign draws
  with an X is a container with its own inner transform; making that explicit
  in the model is what "content-within-frame" above depends on.
- [ ] **Object styles**, cascading on edit the way paragraph styles do.

---

# Milestone 6 — Prepress and Export

**This is what separates a drawing tool from a publishing tool.** A layout
that cannot produce a correct PDF/X for a commercial printer is not a DTP
application.

### Acceptance

> Run preflight and see overset text, missing links, low-resolution images and
> RGB objects in a CMYK document, each clickable to jump to the offender.
> Export PDF/X-4 with crop marks, bleed and registration marks. Open it in
> Acrobat's output preview and confirm the separations, the trim and bleed
> boxes, and the embedded output intent. Hand it to a commercial printer and
> have it RIP correctly. Package the document and get one folder holding the
> file, its links and its fonts.

- [ ] Preflight engine, independent of the GPU, live as the document changes.
- [ ] Preflight rules: overset text (tail-of-thread only), missing and
  modified links, low resolution, colour-space mismatch, missing fonts,
  objects outside the bleed.
- [ ] Preflight panel with click-to-jump, errors sorted above warnings.
- [ ] **A live preflight indicator in the status bar**, so the document's
  state is visible without opening the panel.
- [ ] PDF/X-1a and PDF/X-4 export.
- [ ] CMYK conversion through the document's output intent.
- [ ] `MediaBox`, `TrimBox`, `BleedBox`; crop, bleed and registration marks,
  and colour bars.
- [ ] Font subsetting verified by RIP, not only by Acrobat.
- [ ] Export presets, saved and reused.
- [ ] Package: collect the document, `/Links` and `/Document Fonts`, with a
  summary of dimensions, fonts, required inks and preflight state.

---

# Milestone 7 — The Workspace

**Deliberately late.** Building workspace chrome before the application could
keep a user's work is the exact mistake this plan corrects. Until this
milestone the layout is fixed: a tool strip, one inspector, and the canvas.

### Acceptance

> Drag a panel to another edge, group it into a tabbed stack, resize it by its
> splitter, and collapse it to an icon rail. Save that arrangement as a named
> workspace, switch to another, and switch back. Quit and relaunch, and find
> the layout as it was. Drive the common operations from the keyboard, and
> from the menu bar, without reaching for the mouse.

- [ ] Dockable panels: drag between regions, tabbed stacks, splitters, icon
  rail. **Floating panels remain out of scope** (see the spec, section 14).
- [ ] Named workspaces, saved and restored, with presets.
- [ ] Full menu bar with accelerators.
- [ ] A keyboard shortcut for every common command, user-remappable.
- [ ] Theme tokens complete; light and dark both finished → **moved to
  milestone 1.5, phase A**, with contrast asserted by a test.
- [ ] Preferences dialog. *The store itself lands in milestone 1.5 phase A,
  because phase A introduces two preferences and they need somewhere to live.*
- [ ] Autosave and crash recovery → **moved to milestone 1.5, phase A.** Data
  safety belongs with the cross-cutting rules, not eight milestones away.
- [ ] Multiple open documents. *The structure lands in milestone 1.5 phase A;
  what remains here is the tab bar and switching between them.*

---

# Milestone 8 — Distribution

### Acceptance

> Download Tessera for Linux, Windows or macOS, install it the way that
> platform expects, launch it, and open a `.tessera` file by double-clicking
> it in the file manager.

- [ ] AppImage or Flatpak, MSI, DMG, built by CI.
- [ ] File-type association and application icon on each platform.
- [ ] **Linux verified interactively** — Wayland and X11, fractional scaling,
  IME, native dialogs.
- [ ] **macOS verified interactively** — Retina, menu bar conventions, IME.
- [ ] Signing and notarization where the platform requires it.
- [ ] Automatic update check.
- [ ] User documentation and a first-run tour.

---

## Working agreement

- Milestones are ordered by dependency. **Milestone 0 is the spine** — the
  application can keep a user's work before it can do anything impressive with
  it.
- Never report progress as a count of ticked boxes. Report which sentences a
  person can now perform. Counting boxes is what hid the last gap.
- Every change lands with its tests, in the same commit as the checkbox it
  moves.
- GPU-backed tests run alone and in the foreground, never inside
  `cargo test --workspace` — adapter acquisition hangs intermittently on this
  hardware, and a hang looks exactly like a slow compile.
- When something is verified on Windows only, write that down. "Verified on
  Windows" and "done" are different words.
