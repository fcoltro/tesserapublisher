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

> **Reconciled 2026-09-07.** Three items here were built and never ticked
> — the reference point, align and distribute, and the light theme in
> milestone 3 — which made this file over-report what was left and useless
> for deciding what to do next. A roadmap that is wrong in the safe
> direction is still wrong: it hides finished work and buries the real
> gaps among imaginary ones. Each claim below was checked against the code
> rather than against memory.

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
- [x] Clipboard, duplicate, and step and repeat.
  - **The offset accumulates.** Each copy is `n` steps from the original, not one
    step from the copy before it: reading the previous copy's position compounds
    any rounding, and a row of forty drifts visibly by the end.
  - One command rather than a loop of Duplicate at the call site, because forty
    copies is one thing somebody did and must be one undo entry. Undoing a row of
    forty forty times is not undo.
  - The box says how many objects there will be **before** it runs. The original
    is not one of the copies, and that off-by-one is the one somebody finds after
    laying out a sheet of labels rather than before.
  - The count is bounded. It is typed, and a stray keystroke turning 12 into
    12000 would make twelve thousand frames and take the application with it.
- [x] Z-order: bring forward, send backward, to front, to back — correct for
  multiple selections, which needs opposite traversal orders per operation.
- [x] Grouping and ungrouping, including nested groups.
- [x] Numeric transform fields with drag-to-scrub, including rotation.
- [x] Every gesture records exactly one undo entry, on completion.
- [x] **Reference point**: transforms resolve about a chosen one of nine
  anchors, which subsumes the from-centre scaling missing above.
- [~] **Shear**, with an honest affine decomposition replacing
  `Transform::rotation_degrees()`'s assumption that no shear exists.
- [x] **Align and distribute** across a multiple selection. Nineteen actions,
  each reachable from the Object menu, the palette and the canvas toolbar.
- [x] Corner options and corner radius. Rounded, bevelled and inverse, with a
  radius per corner.
  - **One outline, built once.** `Corners::outline` returns the path and the
    renderer and the PDF writer both draw *that*. A rounded corner computed
    twice is two corners that agree until somebody fixes a rounding error in
    one of them, and then the export stops matching the screen in a way nobody
    sees until it is printed.
  - Four radii, one shape. A card with one cut corner is a real thing and a
    single radius cannot say it; a rectangle rounded at the top and bevelled at
    the bottom is not something anybody has asked a layout tool for, and the
    control would cost more than the feature.
  - **Clamped on read, not on write.** A radius bigger than half the shorter
    side folds the outline through itself and draws a bow tie, so it is limited
    when the path is built — and the number somebody typed is kept, so making
    the frame bigger again brings the corner back.
  - The panel shows one field while the corners agree and four when they do
    not. Four for the commonest case is three fields of noise; one for a frame
    with different corners is a control that flattens them silently.
  - A cut corner strokes on its own centre line. Offsetting a curved path is an
    offset curve, which is not a bezier and cannot be had by moving control
    points — both drawers make the same compromise, so they still agree.
  - **Format 19, with no migration step, on purpose.** The field defaults to
    square, which is exactly what every document written before it meant. A step
    that touched every frame to write the value it would already read as could
    only introduce a bug.
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

> **Status 2026-09-04: ten of ten done. Complete.**
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
> Every task is built and the one visual check A9 was waiting for has been
> made. Phase B is next, and it is planned separately.

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
- [x] **A9 — Icon cache.** Lucide paths parse to `BezPath` once and are cached
  by `Icon`, instead of being re-parsed on every paint. *Verified by eye on
  2026-09-04: a screenshot of the running application shows the tool strip
  drawing correctly through the cache.*
- [x] **A10 — Autosave and crash recovery.** A periodic atomic write to a
  recovery path, detected and offered on the next launch. Data safety belongs
  with the cross-cutting rules, not at milestone 7.

## Phase B — The Page

> **Status 2026-09-04: complete. All eight built; B4 dropped deliberately.**
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

> **Status 2026-09-04: all three parts built. Twelve of fourteen items done.**
>
> 462 tests pass and clippy is clean at `-D warnings`.
>
> **Two bugs in this work were found by using the application, not by the
> suite** — page setup not invalidating the resolve cache, and autosave
> failing on a directory that had never been created. Both now have
> regression tests at the level the failure was really at. That is the
> argument for the hand checks, made concrete.
>
> **Two items stayed partial for reasons outside phase C, and both are now
> closed.** C10 could show the page count and not navigate, because there had
> only ever been one page; C12's Layout menu did not exist, because the menu
> bar is generated from the action list and Layout had no actions. Milestone 3
> phase 1 supplied the pages and the commands, and both were ticked there.
> Window is still absent, and still for the right reason: it has no commands,
> and a menu entry for an unbuilt feature is the lie this codebase was rebuilt
> to stop telling.
>
> **The sentences have not been performed by hand.** Three sessions of real
> use found four bugs the suite missed — the resolve cache never invalidating
> on a page-setup change, autosave failing on a directory nothing created, a
> click in the inspector ending an on-canvas edit, and a recovery file
> surviving a clean quit. Every one is fixed with a regression test at the
> level the failure was really at. That is the argument for the hand checks,
> made four times over.

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
- [x] **C6 — Align and distribute**, to the selection, the margins, the page
  and the spread — every target reachable from the Object menu and the command
  palette, with a test asserting each one is in the action list by name.
- [x] **C7 — Canvas toolbar**: six aligns, two distributes, two flips and two
  quarter-turns, beside the object, appearing only for two or more, in real
  Lucide glyphs. Placement is tested against the window's edges.
- [x] **C8 — Rulers**, with a 1-2-5 tick ladder, a unit selector that saves
  the preference, and a guide you can drag off either one — dropped back on a
  ruler, the drag is cancelled (B3). A placed guide can be grabbed, moved as
  one undo entry, and thrown away by dropping it off the canvas. The zero
  point is a widget: drag it onto the page to count from there, double-click
  it to put it back on the page's own corner.
- [x] **C9 — Screen modes**: Normal, Preview, Bleed and Slug, all four,
  reachable from the View menu and the palette, with `W` toggling the first
  two. A printing mode hides the handles, frame edges, rules, rulers, guides
  and canvas toolbar, paints the surround the fixed neutral grey of D8, and
  **crops the document to what it reveals** — so Preview shows the trim as it
  will print rather than merely hiding the furniture around it.
- [x] **C10 — Status bar**: a zoom that can be typed, stepped along a
  1-2-5-ish ladder, or fitted, beside the message area, and previous / "3 of
  12" / next. *Closed by milestone 3 phase 1, which is what it was waiting
  on: there had only ever been one page to be on.*
- [x] **C11 — Command palette** over the `Command` enum (A6), showing each
  command's shortcut beside it.
- [x] **C12 — Menus**, generated from the same action list the palette reads,
  so a command cannot be in one and missing from the other. File, Edit,
  Layout, Object, Type and View. *Type arrived with the styles window and
  Layout with milestone 3's pages — in both cases by adding the commands, not
  the menu. Window is still absent, and still because it has no commands: a
  group with no actions gets no menu, and a menu entry for an unbuilt feature
  is the lie the previous codebase told often.*
- [x] **C13 — Icon set**: 31 Lucide glyphs, up from 14, **converted
  mechanically from the official package's own SVG geometry** rather than
  transcribed — the icon tests prove a path parses and that it fits the grid,
  neither of which proves it is the glyph it claims to be. Every surface that
  shows a picture now shows a real one: the tool strip, the canvas toolbar,
  the fill and stroke proxy, the zoom controls.
  *The spec guessed "roughly sixty". That number was wrong, not the work:
  there are 31 places a picture beats a word, and the remaining surfaces —
  the palette, the menus, the inspector's fields — are lists of text where an
  icon would be noise. Padding to sixty would have meant adding glyphs nothing
  draws.*

**Explicitly not in M1.5:** corner radius, opacity, effects, object styles,
gradients, the swatches panel, text wrap, frame fitting, text-frame options,
image links, parent pages, the pages panel, the layers panel, snapping,
dockable panels, document tabs.

---

# Milestone 2 — Typography

The reason a layout tool is not a drawing tool.

> **Design approved 2026-09-05:**
> [milestone 2 typography](docs/superpowers/specs/2026-09-05-milestone-2-typography-design.md).
>
> The one-line version: `Story` holds a single `TextStyle` for all of its
> text, so nothing in the model can express a bold word. That is the whole of
> this milestone'''s difficulty.

### Acceptance

> Set a paragraph in a chosen family, weight, size and leading. Adjust
> tracking and kerning. Set alignment and justification, indents, and space
> before and after. Define a paragraph style, apply it to several paragraphs,
> change the style, and watch every one of them update. Type in a language
> that needs an IME and see the composition preview on the canvas.

> **Complete, 2026-09-05**, but for the two items named below. 697 headless
> tests and 6 GPU-backed ones pass; clippy clean at `-D warnings`; formatted.
> **The acceptance sentence has not been performed by hand.**

- [x] Font enumeration and family/style resolution across all three platforms.
  `Shaper::families()` enumerates fontique, sorted and deduplicated, built on
  first ask. Two tests name `std::env::consts::OS` in their failure, so CI
  answers for ubuntu, windows and macos rather than one machine speaking for
  all three. A family the machine lacks is substituted by parley and **marked**
  in the inspector.
- [x] Character formatting: family, weight, style, size, leading, tracking,
  colour, case and baseline shift, all settable and all drawn. A story shapes
  run by run.
  - Small caps is **synthesised** — letters that were lowercase set as capitals
    at 0.7 of the size — because a probe found 0 of 191 installed families with
    an `smcp` table. The feature is still asked for, and is the better answer
    where a font has it.
  - Case is a display transform: the story keeps what was typed, so turning All
    Caps off gives back the original capitals.
- [x] Paragraph formatting: alignment, justification, indents, space before and
  after, hyphenation, drop caps. Every one of them drawn.
  - Each paragraph is laid out as its own parley layout, which is what made all
    of it possible: parley measures and aligns a whole layout at once.
  - Hyphenation is **English only**. `hypher` holds patterns per language
    behind features and a story has no language to choose between them; a
    `language` on `CharacterFormat` is what unlocks the rest.
- [x] Paragraph and character styles, with live cascade on edit. Based On, the
  `+` override indicator, Clear Overrides, Redefine Style, Break Link, and a
  delete that folds the style into the text so nothing changes appearance.
- [x] Text selection by click-drag, double-click word, triple-click paragraph.
- [ ] IME composition rendered on canvas -> **moved to milestone 2.5.** It is
  a windowing concern rather than a text-model one, and the only item here
  that no headless test can reach; leaving it in would have made the whole
  milestone unverifiable by the suite.
- [x] Right-to-left and bidirectional text render correctly. Two bugs found by
  writing the tests: an unset alignment was passed to parley as `Left`, so
  Hebrew and Arabic began at the wrong edge, and it is now `Start`. Pure
  right-to-left text round-trips every offset through the screen; mixed text
  cannot promise that — where two directions meet, one place on screen is two
  logical offsets — so it promises the weaker true thing instead.
- [x] Typography inspector panel.

### Asked for by the acceptance sentence, and not built

Both are recorded rather than half-built, because a control that sets a value
nothing honours is worse than one that is absent.

- **Kerning has no control.** The font's own pairs *are* applied — metrics
  kerning, which is what a control would default to — and a test pins that a
  pair is never set wider than its letters apart. What is missing is a
  caret-shaped control for a manual kern, and optical kerning, which means
  computing kerns from outlines and is an algorithm rather than a setting.
  Tracking on one character is not a substitute: tightening the first letter of
  a kerned pair made it *wider*, and there is a test saying so.
- **H&J parameters are not expressible.** parley justifies by adjusting cluster
  advances and exposes no minimum, optimum or maximum for word or letter
  spacing, and no glyph scaling. Justification works as an alignment;
  controlling *how* it justifies means writing justification.

### What this milestone corrected on its way through

Recorded because each was a wrong belief rather than a missing feature.

- `Story::style` held the whole story's formatting and was what the shaper read,
  so runs were built for two phases without being consulted. Removing it at
  format version 7 is what made two live-path bugs *reachable*: the keystroke
  path wrote `story.text` alone, leaving the runs describing a length the text
  no longer had, and formatting applied while a caret was live was undone by the
  next letter typed.
- `ShapeKey` was built from `story.runs`, which do not change when a named style
  does — so "change the style and watch both follow" would have failed silently.
  Keyed on resolved formatting now.
- `ParagraphStyle` had two places for character formatting and the cascade read
  only one, so half of them did nothing.
- A run could straddle a paragraph boundary, and `resolve_run` reads the
  paragraph a run *starts* in — so styling one paragraph restyled its
  neighbour. The trap in the fix was `merge_equal_neighbours`, which folded the
  split runs straight back in the same call.
- Three format-migration tests in phase 2 passed while testing nothing, because
  `rewrite_version_for_test` round-trips through the current model. Every
  migration test since is built from a hand-made archive and checked by
  disabling the migration to confirm it goes red.
- Three later edits failed to match their target and said nothing: the shape
  cache's hyphen reserve was never applied, and `offset_at` never went through
  the offset map. Clippy's dead-code lint caught the first. The second was
  caught by strengthening a test — `straße` uppercases to `STRASSE` and both
  are the same number of bytes, so shaped and stored offsets coincided by
  accident and the round trip could not see the bug. `ﬁ` is three bytes and
  uppercases to two, and it fails on the old code.
- Two tests asserted things that were not true and had to be corrected rather
  than the code: a hyphenated line "stays inside its measure" (parley overflows
  a word it cannot break, hyphenated or not), and every offset in mixed-
  direction text round-trips through the screen (at a direction boundary, it
  cannot).

---

# Milestone 2.5 — Input Methods

**Split out of milestone 2 on 2026-09-05.** Composition is a windowing
concern, not a text-model one, and it is the one piece of typography no
headless test can reach. Keeping it visible as its own milestone beats folding
it into platform work, where an unverifiable item quietly becomes an
unverified one.

### Acceptance

> Type in a language that needs an input method and see the composition
> preview on the canvas, in the frame, in the frame's own font — not in a
> floating box over the top of it. Commit it and find the text where the
> preview was.

- [ ] Composition preview rendered on canvas, in the frame.
- [ ] Candidate window positioned against the caret.
- [ ] **Verified on Windows**, with Linux and macOS recorded as unverified
  until someone has done it.

---

# Milestone 3 — Document Structure

### Acceptance

> Add, delete, duplicate and reorder pages, and undo any of it. Work in
> facing-page spreads. Put repeating elements on a master page, apply it to
> many pages, and override one instance locally without breaking the others.
> Organise objects onto named layers, then hide and lock a layer.

- [x] Pages panel: a visual grid of spreads, with drag-to-reorder. Each page is
  a schematic thumbnail — its margins and the blocks its frames occupy — rather
  than a rendered miniature, because a rendered one needs the GPU and the panel
  must draw while the document is being edited. Dragging reorders spreads, and
  the numbers follow: **the numbering is derived from `spread_order`, never
  stored**, so a reorder cannot leave a page labelled wrongly.
- [x] Add, delete and duplicate pages — **all undoable.** Snapshot undo made
  the inverse free; that it stays free is tested by putting a frame on a page,
  removing the page, and requiring the frame back. Removing the last page is
  refused by the document rather than by the command, so no caller has to
  remember not to ask. Spreads reorder, and the geometry follows.
  - Pages are positioned by `reflow_spreads`: `Page.bounds` is document space
    and the rulers, align-to-page, guides and the PDF's `TrimBox` all read it,
    so the bounds stay the truth rather than being derived by each consumer.
  - Facing pages read 1, 2-3, 4-5. The plan claimed this needed no rule about
    cover pages and a test proved otherwise: page 1 is a right-hand page, so
    the first spread holds one and the rest pair up.
  - Duplicating is a **deep** copy, stories included. Sharing one would make
    editing the copy edit the original, and both pages would look right until
    somebody typed.
- [ ] Facing-page spreads with correct left/right geometry. → **moved to
  milestone 1.5, phase B**, along with page size, margins, bleed, slug, and a
  spread that renders as a spread. Tracked there, not here.
### The remaining work, reordered

Layers come **before** masters, and the reason is not that they are smaller.
A master page is a page whose items appear on the pages it is applied to, and
"appear on" has to mean something in the containment model. Deciding what a
layer is settles that question; deciding it afterwards would mean building
masters twice.

- [x] **Layers are document-wide.** Today a layer belongs to a page —
  `Page.layers` — which makes a frame's page and its layer the *same fact*.
  That is why every frame drawn anywhere ended up on page one's layer, and why
  `layer_at` and `rehome_frame` had to exist to correct it. InDesign's model is
  the other way round: **a layer spans every page**, and a frame's page is
  derived from where it is. Both bugs stop being possible, because a derived
  fact cannot disagree with itself.
  - `Document.layer_order: Vec<LayerId>`, back to front, replaces
    `Page.layers`. Paint order becomes layer-major, which is what makes "this
    layer is above that one" true across the whole document rather than within
    a page.
  - A frame's spread comes from its centre — the page holding it, or the
    nearest page when it is out on the pasteboard.
  - Format version 7 → 8. Per-page layers **merge by position**: layer 0 of
    every page becomes one document-wide layer 0. For every document that
    exists today that is one layer holding everything, which is the truth about
    a document written before layers could be chosen.
  - `layer_at` and `rehome_frame` are **deleted**. New frames go on the active
    layer wherever they are drawn, and moving a frame changes nothing about
    which layer it is on — which is correct, and is the InDesign behaviour.
- [x] Layers panel: named layers, reorder, visibility, lock, and an active
  layer that new objects go onto. Reads **top down**, which is the opposite of
  `layer_order`, because the panel is a picture of a stack seen from the front.
  The reversal lives only in the panel.
  - `selectable_order` is `top_level_order` minus the locked layers, and the
    two differ deliberately: a locked layer is **drawn** and not **touched**,
    which is the whole use of locking one. A click passes through it to what is
    underneath, and select-all does not reach it — a chord must not undo a
    deliberate lock.
  - Hiding or locking a layer makes the selection let go of what is on it.
    Otherwise handles stay drawn around something invisible and the next drag
    moves what cannot be seen.
  - Choosing which layer to work on is saved but is **not an undo step**.
    Undoing a rectangle should remove the rectangle, not first take back the
    click that chose where to draw it.
  - The last layer cannot be removed, refused by the document rather than by
    the caller — the same rule the last page has, for the same reason.
- [x] Master pages, rendered behind page content. Applied by **reference, not
  by copy**: a master whose items were copied onto each page would not update
  the pages when it changed, which is the entire reason to have one. There is a
  test that edits a master item and requires the page to show the change, with
  the frame count unmoved.
  - A master is **a spread that is not in the reading order**. It holds pages
    like any other spread and they hold frames like any other page, so layers,
    text and transforms all work on it without knowing what it is.
  - Laid out **above** the document, at negative y. It is drawn there like any
    other spread, because a master you cannot see is a master you cannot edit —
    and its items appear again on each page built on it, which is what applying
    one looks like.
  - A page takes the master page on its **own side of the fold**: a verso takes
    the verso, a recto the recto. A master with different inside and outside
    margins is useless otherwise.
  - Applied by click rather than by drag. A drag needs a visible target and a
    gesture that can be got wrong; the current page is already marked, and
    clicking the parent is one action with one meaning. Clicking the parent a
    page already uses takes it off.
- [x] Master item override, promoting one item to a local editable copy. The
  copy lands exactly where the master item appeared, so overriding changes
  nothing until the copy is edited — it is a promotion, not an edit.
  - The relationship lives on the **document**, not on the frame: an override
    is a relationship between two frames and belongs to neither of its ends.
    A `SecondaryMap`, because the round-trip test pointed out that JSON object
    keys must be strings and a `FrameId` is not one.
  - Removing a master **keeps** what was overridden from it. An override is an
    ordinary frame by then, and deleting somebody's work because a parent went
    is a surprise no undo should have to fix.
  - Format version 8 → 9. Nothing to rewrite, and this time the defaults really
    are the truth: a document written before parents existed has none, none of
    its pages is built on one, and nothing overrides anything.
- [ ] Document setup: page size, orientation, margins, bleed, slug. → **moved
  to milestone 1.5, phase B.** Too much stands on it to leave it this late:
  rulers, screen modes, align-to-page, `TrimBox` and `BleedBox`, and
  preflight's out-of-bleed rule. Per-page size overrides stay here.
- [ ] Screen modes Bleed and Slug → **moved to milestone 1.5, phase C**, since
  phase B supplies the geometry they need.

---

# The interface

Not a milestone: it cuts across all of them, and it was rebuilt once real use
showed the panels were correct and homeless. The record is here so the reasons
travel with the code.

- [x] **Tokens.** Three surface values, four spacing steps each with a stated
  job, one row height, one label column, three type sizes. The three spacing
  constants before this had no rule about which applied where, so one
  relationship was drawn at three sizes in three panels.
- [x] **A twelve-step scale**, Radix's, with the role of every step asserted
  rather than described. The neutral is warm, following Linear off blue-grey:
  a cool grey beside a page proof makes warm paper look yellow, which is a
  judgement the interface must not make for the user. Writing it against the
  contrast tests found four real faults by measurement that the eye passed.
- [x] **A docked rail.** Pages, Layers and Styles were floating windows that
  overlapped the inspector and hid the work. They are sections of one column
  the canvas is laid out beside; collapsed, the rail is a strip of icons
  rather than nothing.
- [x] **A control bar.** One row under the menu describing whatever is
  selected. Geometry moved out of the inspector and lives here alone.
- [x] **Information design in the inspector**: fields fill their width, pairs
  sit side by side, sections collapse and remember, group labels are a weight
  below section headings.
- [~] **A light theme, and a density preference.** The light theme is built and
  switchable; the density preference is not. Both were already decided —
  the light palette is defined and contrast-tested, and `ThemeChoice` is saved
  — and both are blocked on the same thing: the tokens are compile-time
  constants, and a theme that can change while running needs them read at
  runtime. That is a wide, mechanical change to every `Theme::` use, and it
  should be made in one pass rather than half-made.
  - Adobe's finding is the reason to do it at all: on a professional tool,
    density and contrast are a **preference, not a constant**.

### What the design argues from

Alan Cooper's *About Face* names what Tessera is: a **sovereign** application,
one a person works inside for hours with nothing else competing for the screen.
His guidance for those runs against the usual advice, and it is why this
interface is dense, muted and maximised rather than airy and colourful. The
full argument, with sources, is in `docs/superpowers/specs/`.

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
- [x] Ruler guides → **moved to milestone 1.5**: the data to phase B, the
  drag-out to phase C. Margin guides are drawn by phase B; **column guides
  landed here**, and are done.
  - They divide the **type area**, not the trim: a layout is built against its
    margins, and guides dividing the page would put a column under one.
  - Drawn as the sides of each column rather than as boxes. Their tops and
    bottoms lie on the margin rule already, and stroking them again doubles a
    line meant to be a hairline.
  - Objects snap to them, which is most of what a guide is for.
  - **One implementation of how columns divide**, shared by a page's guides and
    a text frame, with a test that the two agree. Two would eventually differ,
    and a frame that did not line up with the guides it was drawn against would
    be a very confusing thing to debug.
  - Zero columns and one column mean the same thing — one, so no interior
    guides — which is what lets the field default cleanly and what a document
    written before them reads as.
- [x] Snapping solver with a pixel-threshold lock and visible indicators.
  - **The threshold is in screen pixels, not points.** Six points is
    imperceptible at 25% and unshakeable at 800%; six pixels feels the same at
    every zoom, which is what makes a snap read as a magnet rather than a
    fight. The conversion happens at the viewport, where the zoom is.
  - Three parts, kept apart: what a spread offers (trim, margins, guides, the
    other objects), the arithmetic over that list, and the painting. Only the
    first knows what a document is and only the last knows what a pixel is, so
    the middle is testable without either.
  - Edges **and centres**: lining two objects up by their middles is as common
    as by their left edges and much harder to do by eye.
  - The axes are solved apart, which is what lets an object settle its left
    edge on a margin while its top stays where the pointer put it.
  - The same arithmetic runs on the preview and on the command that ends the
    drag. Settling only the preview would let the object jump off its line the
    moment the mouse came up, which is worse than no snapping — the user
    watched it line up first.
  - Held off by Ctrl, and turned off for good from the View menu.
- [x] Baseline grid with a per-frame lock toggle.
  - Measured from the top of the **page**, not the frame. That is the whole
    point: two columns in different frames line up because both sit on the
    page's rhythm, and a grid measured per frame would give each frame a
    rhythm of its own. `resolve` converts it into the frame's space, because
    the text crate has no notion of a page.
  - The lock is **per frame**, because a caption or a pull quote is exactly
    the thing that should not share the body text's rhythm.
  - A locked line takes the slot **at or below** where it fell, never the
    nearest: text must not ride up into the line above it.
  - Two lines may not share a slot. Leading tighter than the step would
    otherwise round both onto one line and draw them over each other.
  - A grid **overrides** vertical justification. Both decide where a line
    sits and a line cannot be in two places; the grid wins because it is the
    one that makes separate frames line up.
  - A rotated frame is left off the grid rather than guessed at. A rhythm
    measured down the page means nothing to text running across it at an
    angle.
- [x] Multi-column text frames with gutter control, **frame inset, and
  vertical justification**.
  - Justification is applied **per box, after** the lines are handed out. It
    cannot be done while placing them: where the slack is depends on how many
    lines the box ended up with, and that is not known until the box is full.
    A test requires the same lines to land in the same boxes under all four
    alignments.
  - `TextLayout` lives on the **`Text` variant**, not on `Frame`: a column
    count is a fact about a text frame and a nonsense about a rectangle, and
    the kind is what decides which.
  - The text is shaped **once**, at a column's width, and the lines are then
    handed out. Every column of a frame is the same width, so columns cost a
    cheap pass over a finished layout rather than a shaping each.
  - `ShapedLine` gained an ascent and a descent. A `PositionedGlyph`'s `y`
    **is** its baseline — the ink's extent is not in it — so a flow that
    measured the glyphs found every line zero high and clipped the first line
    of every column by exactly its own ascent. Found by a test, not by eye.
  - The same routine will thread frames: filling a sequence of boxes in order
    is one operation, which is why it takes `Column`s rather than columns.
- [x] **Text threading**: overflow flows to the next frame, and a resize
  reflows the whole chain. Both are tested, the second against the milestone's
  own sentence.
  - A **forward link only**. The frame before is found by looking for whoever
    points here, so a chain has one description rather than two that can
    disagree — the class of bug this codebase has already paid for twice.
  - Threading makes two frames **share one story**. That is what threading is:
    one story shown across several frames, not several stories in a row.
  - Refused when it would make a loop, when either frame holds no text, and
    when the target already takes overflow from somewhere else. A frame with
    two sources would have to show two stories at once, and `thread_of` is
    cycle-safe besides, because a chain that ate itself would hang the render.
  - A frame in a chain is laid out **from where the frame before it stopped**,
    and there is no shortcut past that: how much a frame holds depends on its
    own measure and its own columns, so the frames before it really are laid
    out to find the answer. `shape_from` breaks the remainder afresh at the new
    measure, which is why a thread cannot be a slice of one layout.
  - Unthreading separates the flow without deleting the words: both halves keep
    the story between them.
- [x] **Thread connector lines drawn on selection.** Out of the foot of one
  frame and into the head of the next, so the line says which way the text
  runs. A blob at each end, so a connector running off the edge of the canvas
  still says which frames it joins.
- [x] Text wrap around objects.
  - **parley can already do this.** `set_line_x` and `set_line_max_advance`
    are per line and `break_next` reports each line's top and bottom, which is
    everything a shaped region needs. This codebase was already using the same
    breaker for drop caps. Nothing had to be added upstream.
  - The setting lives on the **obstacle**, not on the text: an object is given
    a wrap once and every frame near it obeys, which is what "wrap text around
    this picture" means. On the text it would mean telling each frame about
    each object.
  - **One run per line**, the widest gap. A line split either side of an
    object is a different line-breaking problem rather than a narrower
    measure, because parley sets one `x` and one advance per line. This is
    InDesign's "largest area", and it is what a designer wants nine times
    in ten.
  - A line's height is a **guess** until it has been broken — it depends on
    what ends up on it — so the band is taken from the line before, which is
    exactly right whenever the leading does not change.
  - The bounding box, not the contour. Wrapping to an outline needs the shape
    intersected with each line, which is a separate piece of work; this is
    InDesign's "wrap around bounding box" and is what most wraps are.
    **Not built: contour wrap, and text on both sides of an object.**

---

# Milestone 5 — Colour and Assets

### Acceptance

> Place a photograph, move and scale it inside its frame, and fit it to the
> frame proportionally. See its effective PPI and get a warning below 300.
> Replace the file on disk and watch the link update. Define a CMYK swatch and
> a spot colour, apply them, edit a swatch, and see every object using it
> change. Assign a document ICC profile and see a soft proof on screen. Fill a
> shape with a gradient and give it a drop shadow.

**Every sentence is performed, and two things are owed.**

The whole sentence is reachable in the application: Ctrl+D places, the Artwork
section reports the effective PPI against a threshold that is a preference, the
link status has three states rather than two, the Swatches panel (F6) defines and
edits global colours, an ICC profile is chosen in document setup and Ctrl+Y shows
the proof, and the Fill section offers a linear or radial ramp while Effects
offers the shadow.

What is owed, stated rather than ticked:

1. **The hand check.** Every claim here rests on tests, and tests cannot see. A
   proof is judged by eye against a printed sheet, a gradient by whether it bands,
   a shadow by whether it reads as depth. That check is a person’s job and has not
   been done.
2. **The two — items below.** Little CMS is confirmed building on Windows only,
   and the drop shadow is not written to the PDF — which the panel says, where a
   person can read it, because a shadow that appears on screen and not in the
   export is the surprise that reaches a printer.

- [x] Image placement with linked (never embedded) assets. Placed with
  Ctrl+D, drawn from the file on disk, and decoded once rather than every
  frame.
  - The same path placed twice is **one** link. Two would be two entries in the
    links panel for one file, two things to relink, and two chances to disagree
    about whether it is missing.
  - A link records the file's **natural size**, so a document opens and lays
    out without touching the disk: a missing image must not stop a page being
    drawn.
  - `resolve` hands on the **path**, not the pixels. Decoding belongs to the
    renderer, which can cache it, and the PDF writer wants bytes rather than a
    decoded surface — handing both a decoded image would decode twice and
    cache neither.
  - **Placed artwork is written to the PDF.** It was not, until now: the
    writer skipped `ResolvedKind::Graphic` entirely, so a page of photographs
    exported as a page of nothing — and because the placeholder is deliberately
    never written either, the file came out looking finished and empty. A JPEG
    is passed through as `/DCTDecode`, which *is* JPEG: the file's own bytes are
    smaller than anything a re-encode could produce and exactly as good. Alpha
    becomes an `/SMask`, since PDF has no RGBA and dropping it would composite a
    cut-out onto black. One file placed forty times is one image object.
  - **PDF/X-1a is refused for a document with pictures in it.** The artwork is
    embedded in `/DeviceRGB` and X-1a admits only CMYK, grey and spot. Converting
    it through the output intent is owed; claiming conformance it does not have
    would be the exact lie the rest of this exporter refuses.
  - The placeholder is **never written to the PDF**. A violet cross in a
    printed job is far worse than a blank space.
  - The decode cache is keyed on the file's **modification time as well as its
    path**, so replacing the file on disk shows the new artwork without anybody
    being asked to reload. A cache keyed on the path alone would happily show
    last week's photograph forever.
  - It is bounded by **total pixels**, not entry count. Ten thumbnails and one
    poster are very different amounts of memory, and a limit that cannot tell
    them apart either wastes room or thrashes.
- [x] Content-within-frame: independent inner transform, fit and fill modes.
  - Fit is an **operation, not stored state**. What persists is the transform
    it produced; storing the mode as well would be a second description of the
    same fact, and the two would disagree the moment somebody nudged the
    picture by hand.
  - Every fit centres what it places. "Fit" without "centre" leaves the slack
    on two sides rather than four, and there is a test over all four modes.
- [x] Clipping of raster content by its container shape. The clip is what
  makes a crop a crop: content larger than its frame is cut by it rather than
  spilling onto the page.
- [x] Link status: OK, missing, **and modified** — with relink and update.
  - **Three** states rather than two. "The file has changed" is the one the
    previous codebase never drew, and the reason somebody could send a printer
    last week's photograph.
- [x] Effective-PPI reporting with a configurable warning threshold.
  - **Effective**, not natural: a 300ppi photograph scaled to twice its size is
    a 150ppi photograph, and the effective figure is the one a printer cares
    about.
  - Two figures when the axes differ, because a stretched placement really does
    have two and a single number would hide it.
  - The threshold is a **preference**, not a constant: 300 is the bar for
    offset litho, 150 is fine for newsprint, and 72 is right for a screen PDF.
    A hard-coded 300 would cry wolf at every newspaper.
- [x] Disk-backed proxy cache, so downscaling survives a restart.
  - The in-memory cache stops a decode per *frame*; this stops one per *restart*,
    which is the wait a person actually notices on opening a picture-heavy
    document. A test proves it: one session writes, a second reads back and
    decodes nothing.
  - **Raw RGBA, not PNG.** A proxy is written once and read on every cold start,
    so reading fast matters and writing small does not. Re-encoding on write and
    inflating on read would trade away the one thing it exists to buy, and the
    files are the cache’s own so nothing outside has to read them.
  - Keyed on the file, its modification time **and** the size asked for. The
    modification time is there for the same reason as in the memory cache — a
    cache that served last week’s photograph would be worse than no cache — and
    a test replaces a file on disk to prove it.
  - Sizes are **rounded up to a power of two**, so nudging a frame by a point
    does not throw the proxy away and build another. A dozen sizes per picture
    would be a cache that never hits.
  - The **original size is never written**. Putting the full pixels of every
    photograph in a cache directory would be a copy of the user’s picture
    library, and a resolution report or an export asks for the original anyway.
  - It lives in the platform’s *cache* directory, not beside the preferences:
    everything in it can be rebuilt, so the system is welcome to delete it, and
    asking a backup to carry derived data — or losing settings because a cache
    was cleared — would both be wrong.
  - A truncated or foreign file is **discarded and rebuilt**, never misread. A
    half-written file that a later read trusts and draws as garbage is the worst
    failure a cache has, so proxies go out through a temporary and a rename and
    are checked against their own header on the way back in.
  - A cache that cannot be written makes Tessera **slower, not broken**: a
    read-only directory or a full disk is a missed optimisation, not an error.
  - Where the cache lives is a **field, not a global**, which is both how a
    portable install can place it and how "today and tomorrow" is testable
    without an environment variable or an ordering dependency between tests.
- [~] `lcms2` integration — **built and working; confirmed on Windows only.**
  Little CMS is vendored and compiled from source by `lcms2-sys` rather than
  linked against whatever the machine happens to have, so a build is
  reproducible and needs no system package. The `unsafe` stays inside those two
  crates and nothing in this workspace gains any, so `unsafe_code = "forbid"`
  still holds workspace-wide.
  - **What is owed:** a build on macOS and on Linux. It compiles here with the
    MSVC toolchain; the other two need a machine or a CI runner, and claiming
    them from a Windows box would be exactly the kind of unverified tick this
    roadmap exists to avoid.
- [x] RGB, CMYK, Lab and spot colour throughout the model. RGB, CMYK and spot
  were built in milestone 0 for exactly this moment; Lab and the swatch
  reference landed here.
  - Lab converts through **D50**, the illuminant a printing standard assumes,
    and is the plain formula — a placeholder for the ICC transform in the same
    documented way the CMYK conversion has been since milestone 0.
- [x] Global colours that cascade on edit. Objects store the swatch's *name*, so
  editing it changes every one of them without any being touched, and there is a
  test that does exactly that.
  - The panel is a list of the document's **definitions**, not a palette to pick
    from. That is what a swatch is: rename one and every object follows.
  - Deleting one **says what it costs first**. "Remove Brand red" is a different
    decision when four objects use it than when none do, and a panel that does
    not say which is asking somebody to guess.
  - The chosen swatch is held **by name, not by index**. The list is reordered by
    every rename, and an index would quietly start pointing at a different
    colour.
  - A new swatch takes the **selected object's colour**, because naming the colour
    you are looking at is what "new swatch" almost always means. With nothing
    selected it is a plain black — a colour rather than a surprise.
  - Only the swatch being worked on carries a picker. Every row carrying one
    would be a column of pickers, and one is edited at a time.
  - `Color::Swatch` carries **no fallback**, deliberately. A fallback is a
    second copy of the value, and the second copy is what a global colour
    exists to avoid — so a swatch cannot resolve itself and a document must be
    asked.
  - An unresolved swatch draws in an alarming magenta rather than black.
    Drawn in black it would look like a decision; drawn in that it looks like
    what it is.
  - Deleting a swatch **leaves the references unresolved** rather than baking
    in its last value, which would silently keep a colour the user had just
    deleted.
  - A ring of swatches stops rather than hanging: somebody who points A at B
    at A has made a mistake and should see an unresolved colour, not a frozen
    application.
  - `resolve` flattens every swatch, so the renderer and the PDF writer never
    meet a name and stay ignorant of the document.
- [x] Document output intent with on-screen soft proofing.
  - **The profile travels in the document, not a path to it.** A layout recording
    "C:/profiles/FOGRA39.icc" means something different on the printer’s machine
    than on the designer’s, and that is exactly where being wrong is expensive.
    PDF/X requires the profile in the file anyway.
  - `None` is **"nobody has said"**, not "sRGB by default". Inventing a profile
    would show every older document proofed against a decision its author never
    made, and the colours would be believed.
  - **A CMYK colour is converted, not proofed.** Sending 100% cyan through the
    naive formula and then round-tripping the result proofs the *formula’s error*
    rather than the press—and 100% cyan is precisely the colour a person
    checks. So a proof holds two transforms: an ink-to-screen conversion for
    colours already in the press’s space, and the round trip for everything
    else. `Proof::show` picks, so no caller has to know there are two.
  - The paper is proofed too. Its white is the most visible thing a proof shows,
    and ink proofed over a pure-white page would look wrong in one direction
    everywhere.
  - **The furniture is not proofed, by construction rather than by care.** Margin
    rules, guides and handles are drawn from the theme’s own constants and never
    pass through a `Color` at all—which is how the plain conversion function
    ended up dead code and was removed.
  - The transform is compiled **once per choice, not once per frame**: building
    one costs more than the conversion it replaces. A counter proves the rule
    holds, because comparing addresses does not—an allocator is free to hand
    back the one it just released.
  - Switching the proof off **keeps it built**, so turning it back on is instant.
    Comparing the two views is the whole way a person uses this.
  - A profile that cannot be used **says why**. Somebody who asked for a proof
    and is not seeing one is entitled to know, so it is read when it is chosen
    — while the dialog is still on screen — and again reported in the panel.
  - Which press is in the **document**; whether you are looking through it is in
    the **application**. The first travels with the file and is what the printer
    needs; the second is a way of working, like the active tool.
  - **The standard profiles are on offer without shipping anybody else’s files**,
    and the two halves of that list cannot be got the same way.
    - An **RGB working space is defined by numbers** — three primaries, a white
      point, a transfer curve, all published — so sRGB, Adobe RGB (1998)
      compatible, Display P3, ProPhoto RGB, Rec. 2020 and two greys are *built*
      from those numbers. Nothing bundled, nothing downloaded, colorimetrically
      exact. The sRGB and Rec. 709 curves go in as their published five
      parameters rather than as a rounded gamma: a plain 2.2 is visibly wrong in
      the shadows, which is where a proof is judged, and a test pins the linear
      foot.
    - A **CMYK profile is measured** — thousands of printed and read patches, with
      no formula to compute it from. It has to come from a file, and the familiar
      files (`USWebCoatedSWOP.icc`, `CoatedFOGRA39.icc`) are Adobe’s, under
      Adobe’s copyright, and not ours to redistribute. So Tessera **finds the
      ones already on the machine**: the system’s colour directory, and the
      directories the creative suites install theirs into. On the development
      machine that is 23 CMYK presses including Coated FOGRA39, U.S. Web Coated
      (SWOP) v2, Coated GRACoL 2006 and Japan Color 2001 Coated.
    - Discovery beats bundling twice over: it is legally clean, and a document
      proofed here against "Coated FOGRA39" is proofed against **the same bytes**
      the next application will use.
    - **Some presses *are* freely licensed, and there is a vendoring step for
      them.** `tools/vendor-profiles.py` fetches what
      `assets/profiles/manifest.tsv` names, and the application offers whichever
      of those files is actually present — so an un-vendored checkout has a
      shorter list rather than a broken one. The script **verifies rather than
      trusts**: a download rotted into an error page, truncated, or of the wrong
      colour space is reported and discarded, because an application that proofs
      against nonsense is believed. It also refuses any profile whose licence has
      no entry in `LICENCES.md`, and the same rule is held from the Rust side by a
      test — so a profile cannot be added without its terms.
    - `CANDIDATES.md` records which presses are free and which are not, and the
      distinction that matters: FOGRA39 is a *printing condition*, its
      characterisation data is published, and Adobe’s `CoatedFOGRA39.icc` is
      Adobe’s *build* of it. The strongest candidate to bundle is basICColor’s
      `ISOcoated_v2_bas.ICC` — the same FOGRA39L condition, permissively licensed,
      and already reviewed as DFSG-free by Debian, which is a second party having
      read the terms.
    - **The free CMYK presses are the CGATS.21-2 reference printing conditions.**
      Seven of them — cold-set news through premium coated to extra-large gamut —
      published through the ICC registry, granted to be "used, embedded,
      exchanged, and shared without restriction". CRPC6 is the one nearest the
      coated stock most commercial work is printed on. So the CMYK list is
      answered after all, and the earlier "nothing free exists" was wrong.
    - The catch is one clause: they **may not be sold**. A free build may ship
      them as aggregated data; a build that will be sold, or a Debian package
      which must permit selling, runs the vendoring script with
      `--skip idealliance-crpc`. `LICENCES.md` states the "may be sold" answer for
      every tag, because that is the clause deciding who can ship the result.
    - **Nothing fetched is committed.** The repository carries URLs and terms,
      never profiles, so Tessera’s own source stays sellable by anyone without a
      thought about profile licensing. Naming a licence tag that no row uses is an
      error rather than a no-op: a typo in `--skip` would silently ship what was
      meant to be left out.

    - **The scan recurses, and that was a bug rather than a nicety.** Debian’s
      `icc-profiles-free` installs into `/usr/share/color/icc/basICColor/` and
      `.../OpenICC/`, so reading one level found nothing on exactly the platform
      where the freely licensed presses live. Three levels, symlinks not
      followed, and a cap on files looked at.
    - The search paths include **Krita, Scribus, GIMP, Inkscape and darktable**,
      each of which keeps profiles inside its own installation. Looking there
      costs a `read_dir` that usually fails and gains their whole answer on a
      machine that has any of them.
    - `HOW-OTHERS-SOLVE-IT.md` records how each of those projects handles this.
      They all split it the same way, and the split is forced rather than
      preferred: RGB spaces are shipped or synthesised because arithmetic needs
      no permission, and CMYK presses come from outside the application because
      measured data has an owner. **Scribus** — the closest analogue, open-source
      DTP on littleCMS with real soft proofing — discovers rather than bundles, and
      has for twenty years.

    - **ACES and OpenColorIO are the answer to a different question.** Blender’s
      colour management is OCIO, which is scene-linear working spaces and view
      transforms for rendering and film. It is freely licensed and would be
      legitimate to adopt — and it has **no CMYK output**, so it cannot answer
      "what will this look like on that press". It would earn its place for HDR
      and wide-gamut imagery, alongside ICC rather than instead of it.
  - "Adobe RGB (1998) **compatible**", deliberately. The primaries and gamma are
    published and are what it is built from, so it behaves identically — but
    Adobe’s profile is Adobe’s, and claiming to *be* it is a claim nobody here is
    entitled to make.
  - Greyscale is a real output intent, not an unsupported space: a newspaper
    printed in one ink has one.
  - The machine is scanned **once**, not per frame, with a cap on how many files
    are read and a "look again" for somebody who has just installed one. A menu
    that read the disk on every frame it was open for would be a menu that reads
    the disk.
  - **A bug this found, worth recording.** `Transform::new_proofing` silently
    *ignores the proofing profile* unless `SOFT_PROOFING` is passed: Little CMS
    sees that the source and destination are the same profile and collapses the
    whole thing to an identity. The proof did nothing at all, and it was the worst
    kind of nothing — it looked like a press with a perfect gamut. The test that
    catches it has to probe a press *narrower* than the source, because sRGB
    content proofed for a wider space is correctly left alone; a saturated green
    proofed for a one-ink press must come back neutral.
- [x] Linear and radial gradients; drop shadow; multiply, screen and overlay
  blending — **all three on screen and in the PDF.**
  - A shadow in a PDF is pixels, because PDF has no blur operator and a gaussian
    of a rectangle is not any gradient PDF can express. It waited for the writer
    to be able to embed an image at all.
  - Written as an image of the shadow's colour wearing its softness as an
    `/SMask`, **not** as a luminosity soft mask — that is the other way to do it
    and needs a transparency group and an `/ExtGState` to hang it on, for the
    same picture. This reuses the path placed artwork already goes through,
    which is the path that is already tested.
  - Three box blurs rather than a gaussian: indistinguishable at the sizes a
    mask needs, and each pass is a running sum, so a 144-point blur costs what a
    2-point one does.
  - The mask is built at two samples a point, not at the artwork's resolution. A
    shadow is a soft edge with no detail in it, and a mask at 300ppi for a
    full-page frame is a nine-megapixel greyscale image describing a gradient.
  - It is bigger than the shape it belongs to, by the blur's reach on every
    side. A mask that stopped at the edge would clip the shadow into the one
    thing it must not have.
  - The shadow's alpha is folded into the mask rather than written as a separate
    graphics state: coverage and opacity multiply, so doing it once is the same
    result with one object instead of two.
  - The shadow is drawn **behind the object and outside its composite group**. A
    shadow inside the group would be faded by the object’s own opacity, so a 50%
    object would cast a 25% shadow — and it is the object that is translucent,
    not the light. A test pins that the shadow opens no layer of its own.
  - One colour and **no separate opacity field**. Everywhere else an alpha and an
    opacity are different facts, but a shadow has no fill and no stroke to tell
    apart: its colour *is* how much of it shows, and a second number would be two
    descriptions of one fact.
  - `Option<Shadow>` rather than a shadow at no alpha, because "no shadow" and "a
    shadow turned all the way down" are different things to say.
  - The blur is **capped**, and that is a cost limit rather than a matter of
    taste: a gaussian is evaluated over two and a half deviations either side, so
    a stray value dragged in by accident would stall a redraw rather than merely
    look wrong. Clamped on the way out, so the file still says what it said.
  - Vello can blur a rounded rectangle and nothing else, so how honest a shadow
    is depends on the shape: a rectangle, a picture box and a text frame are
    exact; an **ellipse** takes a corner radius of half its shorter side, which
    for a circle *is* the circle and for a long ellipse is a capsule the blur
    hides the difference in; a **path** gets its bounding box, which is the one
    case visibly not the object.
  - **Not written to the PDF, and the panel says so.** A blurred shadow in a PDF
    is a luminosity soft mask, and a gaussian blur of a rectangle is no gradient
    PDF can express — it has to be a rasterised grey image, which means
    embedding images, which the writer does not do yet either. A *hard* offset
    duplicate would be worse than nothing: a missing shadow is obviously
    missing, and a hard one looks like somebody meant it. A shadow that appears
    on screen and not in the export is exactly the surprise that reaches a
    printer, so it is stated in the interface rather than only here.
  - Only the **separable** modes, deliberately. A non-separable mode cannot be
    reproduced identically on screen and in the PDF, and a mode that looks one
    way in Tessera and another in the file is worse than no mode at all — so
    neither converter has a catch-all arm quietly exporting something as
    Normal.
  - **A gradient is not a colour**, so a fill is now a *paint*. A colour answers
    "what is your value" — every consumer asks it — and a gradient has no single
    answer; a `Color::Gradient` variant returning its first stop, or an average,
    would be a lie told once and believed everywhere. `Paint::solid()` returns
    `None` for a gradient rather than a plausible stand-in, so every caller has
    to say what it does about one.
  - The ramp is an **angle**, not two points, and it is built in the frame’s own
    space. Points would have to be in *some* space: in the document they slide
    out of the object the moment it moves, and in the frame they have to be
    rewritten every time it is resized. A test pins that two frames of the same
    size at different places on the page produce the same ramp, moved.
  - `Gradient::axis` is the **one place the angle becomes geometry**, so the
    renderer and the PDF writer cannot disagree about which way a ramp runs. A
    test asserts the PDF’s coordinates are that same axis, flipped.
  - Stops are held **sorted, and always at least two**. A renderer, a PDF writer
    and a panel each sorting the same list is three chances to sort it
    differently, and a one-stop ramp is a solid colour described the hard way
    that every consumer would have to guard for.
  - Stops hold **colours, not values**, so a gradient can be built out of the
    document’s swatches and editing one changes every gradient using it.
    `uses_of_swatch` looks inside the stops, so deleting a swatch reports what it
    really costs.
  - Vello takes a gradient as a *brush*. PDF has none for the fill operator, so
    the shape becomes the **clip** and `sh` paints it — and because a PDF ramp
    interpolates between two colours per function, an N-stop gradient is N-1
    exponential functions joined by a stitching function. The model’s
    two-stop floor is what stops that arithmetic underflowing.
  - Both ends are **extended**, so the first and last colours run to the edge of
    the shape rather than leaving a corner the flat colour of a ramp that ran
    out.
  - Gradient **strokes** are not modelled. A stroke carries one colour, and the
    two places that have to stand one in for a fill — an open path drawing
    itself, and the fill/stroke swap — take one colour from the ramp and say so
    rather than pretending to draw it along the line.
- [x] **Object opacity** as a field distinct from blend mode, and distinct
  again from a fill colour's alpha.
  - The distinction is the whole point. A fill at half alpha leaves the stroke
    solid, so the stroke shows through its own fill; an object at half opacity
    is composited **once, as a whole** — fill, stroke, artwork and glyphs
    painted into one layer, and the result made translucent. Both are worth
    having: a watermark wants the second, a tinted panel behind opaque type
    wants the first.
  - Opacity and mode are **one command**, so undoing "make this a 40% multiply"
    is one step rather than two.
  - A composite group is opened **only when the object needs one**. Nearly
    every object is plain, and a layer per object would charge every one of
    them for a feature none of them uses; a test pins that an opaque rectangle
    opens no layer.
  - An object at no opacity paints nothing and is not written to the PDF — but
    stays selectable and stays on its layer. This is about ink, not existence,
    and the panel says so where a person can read it.
  - Opacity is clamped **on the way out**, not on the way in, so a document
    carrying a stray value draws sensibly rather than being quietly rewritten.
  - **A stated shortfall in the PDF.** `/ca` and `/CA` are per-paint alphas, so
    a translucent object with both a fill and a stroke has each faded
    separately in the file and its stroke shows faintly through its own fill,
    where the screen composites the object as one group. Closing it needs a
    transparency-group form XObject per object, which belongs with the rest of
    export quality in milestone 6.
- [x] **A graphic frame is not a shape.** `FrameKind::Graphic` is a container
  with contents of its own, and the contents have a transform independent of
  the frame's: moving the frame carries the picture, and moving the picture
  inside leaves the frame alone. Conflating the two is why the previous
  codebase could never move an image within its frame.
  - **Cropping needs no model.** It is what you get when the content's box is
    larger than the container's, which falls out of having two transforms.
  - An empty graphic frame is a **real thing**, not a fault: it is the box a
    designer draws to reserve room for a photograph that has not arrived. It
    draws in the same violet as the column guides, and a frame whose file has
    *gone* draws red — different problems, and only the second is a fault.
- [x] **Object styles**, cascading on edit the way paragraph styles do.
  - **An override survives a style edit, and no override list is kept.** A style
    that simply overwrote its objects would throw away every hand adjustment the
    moment the style changed; one that recorded which properties each object had
    overridden would be a second description of a fact the values already tell.
    So the cascade *compares*: where the style’s old value is still what the
    object holds, the object was following and is updated; where it differs, the
    object was overriding and is left alone.
  - Because of that, "differs from its style" is **asked of the values** rather
    than looked up, and so it cannot be wrong. The inspector names *which*
    properties differ, because "differs somehow" sends a person through every
    control to find out where.
  - A property can be **stated or deliberately left alone**, and where the
    property is itself optional the field nests: `None` is "says nothing",
    `Some(None)` is "says: none". Collapsing them would make "no stroke"
    unstateable, which is exactly what a style for a plain filled box has to say.
    A round-trip test caught JSON quietly collapsing the two — `null` reads
    back as the outer `None` — and a custom reader keeps them apart.
  - `based_on` is a **chain, not a copy**, so editing a base reaches the objects
    of every style built on it, and a ring stops rather than hanging.
  - Removing a style **leaves its objects looking exactly the same**. That is the
    opposite of deleting a swatch, and for the opposite reason: a swatch is a
    value objects point at, so removing it removes the value; a style is a source
    they copied from, so removing it removes only the source.

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

**Nine of ten items performed, and what is owed is not more code.**

*"Have it RIP correctly"* needs a commercial printer and a press. *"Open it in
Acrobat’s output preview"* needs Acrobat. Both are a person’s job, and ticking
them because the file parses would be exactly the unverified claim this
milestone is built to refuse — the same refusal the exporter makes when it will
not write a PDF/X key it cannot stand behind.

The one sentence knowingly **not** met: the folder holds the file and its links,
and *lists* its fonts rather than holding them. A licence to set type is not a
licence to pass the font on. See the packaging item.

- [x] Preflight engine, independent of the GPU, live as the document changes.
  - **A crate, and the boundary is the proof.** `tessera_preflight` cannot reach
    `vello` or `wgpu`, so "independent of the GPU" is enforced by the dependency
    list rather than asserted in a comment. Moving `effective_ppi` out of the
    renderer was the first thing it asked for, and it was right to ask.
  - Keyed on the document’s **revision**, not a timer and not every frame. Shaping
    every story to find the overset ones is the same work laying the document
    out is; doing it sixty times a second for a document nobody is touching
    would be the most expensive thing in the application.
  - Link status is the one thing a revision cannot see — a file can vanish while
    the document sits untouched — which is why the panel has a re-check button
    and not only a list.
- [x] Preflight rules — **all eight.**
  - Missing fonts is asked of the **shaper**, not of a system font list. The
    shaper is what will actually set the type, generic families and fallbacks
    included, so it is the only thing whose answer matches what a reader sees. A
    rule consulting the font list separately would disagree with the renderer
    eventually, and silently.
  - An error, not a warning, and for the reason a missing picture is: the type
    is set in whatever the fallback is, so the copy fits and breaks differently
    and the job comes back looking like somebody else's. Worse than a missing
    picture in one way — a missing picture prints as nothing and gets noticed,
    and a substituted face prints as type.
  - Once per family, not once per frame. A document set entirely in one missing
    face has one problem, not four hundred, and a report nobody scrolls to the
    end of is a report nobody reads.
  - A family named only by the document default or by a style is reported
    against the *document*, because it is not any one frame's fault and a jump
    to an arbitrary frame looks like an answer to "where is it?".
  - The walk that finds the families now lives in `tessera_preflight::fonts` and
    packaging calls it. It had its own copy, and two walks over one fact drift —
    the way they drift is that one forgets the run-local families, which is the
    half a printer would have been misled about.
  - **Errors and warnings are a real distinction**, and the line is not taste:
    it is whether a printer following the file exactly produces something the
    customer did not intend. Overset text, a missing link and an unresolved
    swatch come back wrong, so they are errors. A modified link might be the
    newer artwork somebody meant; 200ppi is fine on newsprint; an RGB object
    might be going to a digital press. Those need a person, so they are
    warnings. The severity is stated on the rule, so one rule cannot be an
    error in one place and a warning in another.
  - Overset is reported for the **tail of a thread only**. A story running
    through four frames overflows the first three by design — that is what
    threading *is* — and reporting each would turn a working chain into four
    errors.
  - The colour-space rule is **only asked when a press has been chosen**, and a
    CMYK press is told from an RGB one by the profile’s header rather than its
    name. Without an intent it reports that fact once, about the document,
    rather than reporting every RGB object in a layout nobody has said is for
    print — which is the noise that gets preflight switched off.
  - The bleed rule is **off entirely for a document with no bleed**, for the
    same reason.
  - **Missing fonts is absent rather than approximate.** "Is this font really
    missing, or is the fallback fine?" has no definite answer today, and a rule
    that guesses is a rule people learn to ignore.
- [x] Preflight panel with click-to-jump, errors sorted above warnings.
  - **Click-to-jump is the whole feature.** Forty problems with no locations is
    a list somebody reads and then has to find everything in twice, and that is
    how preflight comes to be skipped.
  - A row selects the object **and** brings it into view. Selecting alone leaves
    the offender off screen, which is very often why it was not noticed;
    scrolling alone puts somebody in the right place with no idea which object
    was meant.
  - Grouped by rule, because ten low-resolution images are one decision about
    resolution rather than ten discoveries.
  - The sort is **stable**, so an unchanged document gives the same list in the
    same order. Rows that moved between frames would be unclickable.
- [x] **A live preflight indicator in the status bar**, so the document’s state
  is visible without opening the panel.
  - The point of the whole feature. Somebody who has to open a panel to find out
    whether their document is sendable opens it once, at the beginning, and
    never again.
  - **Not green when clear.** A green light invites somebody to stop reading,
    and "no problems" here means "no problems this checks for" — the hand check
    is still owed.
  - Severity is a **shape as well as a colour**: roughly one man in twelve
    cannot tell the red from the amber.
- [x] PDF/X-1a and PDF/X-4 export.
  - **The file refuses to lie.** `GTS_PDFXVersion` is written only after
    `refusals` comes back empty, because a printer’s preflight *believes* that
    key: a file claiming a standard it does not meet passes their check and
    fails on the press instead of in the studio. A claim with nothing behind it
    is worse than no claim.
  - X-1a is refused for a document using transparency. Tessera does not flatten,
    so the claim could not be honoured, and X-4 exists precisely for that
    document.
  - Either standard is refused without an output intent, because PDF/X is a
    promise about *which* press and there is nothing to promise.
  - `/Trapped` is written as unknown, which is the only honest answer: Tessera
    does not trap, and `False` would say the file had been checked and needs
    none.
- [x] CMYK conversion through the document’s output intent.
  - **A CMYK colour is passed through, not converted.** The same trap the soft
    proof had: 100% K is one ink and a rich black is four, and a designer who
    typed one and got the other has been overruled by a colour engine. Only
    colours in some other space are converted.
  - Gradients follow the ink: a shading declares its colour space once and every
    function under it must agree, so a ramp in a CMYK export is four components
    per stop.
  - An unusable profile falls back to RGB rather than failing the export.
    Preflight has already said so, and a readable file beats no file.
  - **A spot is written as its fallback, which is a shortfall.** It should
    separate onto its own plate through a Separation space and a tint transform.
    Recorded rather than hidden.
- [x] `MediaBox`, `TrimBox`, `BleedBox`; crop, bleed and registration marks,
  and colour bars.
  - **A registration mark is 100% of every ink**, which is the only thing that
    makes it work: a mark in black lands on the black plate alone and says
    nothing about whether the other three line up.
  - Marks grow the media box and leave the trim where it is. A media box that
    stopped at the bleed would crop the crop marks — a failure only noticed on
    the proof — and BleedBox stays the bleed, because saying the ink runs as far
    as the marks is a lie a printer acts on.
  - Marks clear the bleed even when the offset is smaller, because a mark over
    the artwork cannot be seen. Marks at no offset are refused: they would be
    cut through by the trim.
  - No colour bar in an RGB export. There are no plates to measure, and a
    printer seeing one would reasonably assume the file was separated.
- [ ] Font subsetting verified by RIP, not only by Acrobat. **Cannot be done
  here.** It needs a real RIP and a real press, which is a person with hardware
  rather than a test. Left open on purpose: ticking it on the strength of
  Acrobat opening the file would be exactly the unverified claim the rest of
  this milestone is built to avoid.
- [x] Export presets, saved and reused.
  - **Not a convenience.** A studio sends the same three kinds of file for
    years, and re-choosing a standard and a set of marks each time is how a job
    goes out as an RGB proof to a printer expecting X-1a.
  - A preset deliberately does **not** carry the output intent. Which press a
    job is for belongs to the document — it travels in the file and is what the
    printer needs — and a preset carrying one would silently re-target somebody’s
    job to the press they last used.
  - The dialog says what the export will be, in a sentence, before it runs.
    Finding out from the file afterwards is finding out too late.
- [~] Package: collect the document, `/Links` and a summary — **fonts are listed,
  not copied, and that is deliberate.**
  - A font is licensed software. A licence to *set type* is not a licence to
    redistribute the file: outline embedding in a PDF is explicitly permitted by
    most foundries and handing over the `.otf` is explicitly not. InDesign copies
    them behind a warning dialog; Tessera lists them so the question can be
    asked, and says in the summary why they are not in the folder. The PDF
    carries subsetted outlines, which is what makes the job printable.
  - A link that cannot be copied is **reported, not fatal**. A job with one
    missing photograph still needs packaging: the printer wants everything else
    and the studio needs the list to chase.
  - The summary is plain text, opened on a machine nobody here chose by somebody
    whose job is to check a folder and pass it on. It carries the trim size, the
    bleed, the press, the spot inks, the links, the fonts and the preflight
    state — a folder that says nothing about its own state is one a printer has
    to check from scratch.

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

- [x] Dockable panels: drag between regions, tabbed stacks, splitters, icon
  rail. **Floating panels remain out of scope** (see the spec, section 14).
  - The arrangement is a **model with no screen in it** — `docking::Docking` —
    and the drawing is separate. That split is what makes the part most likely
    to be subtly wrong the part that is tested: eleven tests, none of which need
    a GPU.
  - **A panel is in exactly one place.** Everything goes through `place`, which
    lifts the panel from wherever it was before putting it anywhere new. Drawn
    twice, the second copy edits the same state as the first, which reads as a
    bug in whatever the panel does rather than in the layout.
  - Panels are stored by **name**, not by index. Numbering them means a layout
    saved by an older build silently reassigns itself when a panel is added in
    the middle of the list: it would still load, and every panel would be in the
    wrong place. An unknown name is dropped and a missing panel is appended.
  - Tabs rather than a column of stacked headings. Six panels under six headings
    puts the one you want below the fold, and opening another pushes it further
    down.
  - A shut panel **keeps its tab**. Closing a panel is not taking it out of the
    layout, and a tab that vanished would mean reopening it from the Window menu
    and finding it somewhere else.
  - Empty stacks are removed and every active index is clamped after every
    change. An empty stack is a splitter with nothing in it; an index past the
    end is a panel area that draws nothing while its tabs say otherwise.
  - The drop targets are the tab bars and a strip at each outer edge, and
    nothing else. Every drop lands somewhere a panel can live, which is what
    "no floating panels" has to mean in the interface rather than only in the
    spec.
  - The layout lives in the **preferences**, which is the whole of what makes
    "quit and relaunch, and find the layout as it was" work: the file that
    already persists is the one it belongs in.
- [x] Named workspaces, saved and restored, with presets. **They carry the
  docking too**, so switching away and back restores the arrangement rather than
  only the list of what was open — the list is not the half anybody arranged.
  `an_arrangement_of_panels_survives_switching_away_and_back` is the acceptance
  sentence as a test.
  - Panels, their order, and the rail. **Not** the theme, the document, or
    where the page was scrolled to: those belong to the person and to the work,
    and a workspace that restored them would mean switching from Layout to
    Prepress turned the lights off and threw away your place on the page.
  - Applying one **closes what it does not ask for**, or switching from a
    crowded arrangement to a spare one leaves the crowd behind and the spare
    workspace is only spare the first time it is used.
  - Panels are stored by name, so an arrangement saved by an older build still
    applies minus the part this build cannot honour.
  - A settings page removes them. A list that can only be added to is a list
    that fills up, and workspaces are made by hand from whatever the panels
    happened to be doing.
- [x] Full menu bar with accelerators.
- [x] A keyboard shortcut for every common command, user-remappable.
  - **The shortcut was written down twice**, and that was the whole problem:
    `actions.rs` carried `Some("Ctrl+N")` as a label and the handler separately
    matched `pressed(cmd, Key::N)`. Two descriptions of one fact, free to
    disagree, with no test able to notice \u2014 and the reason remapping was
    impossible, because changing the string changed the label and nothing else.
  - Wiring the handler to the table found two shortcuts wrong for months.
    Duplicate and Place both claimed `Ctrl+D`; the handler fired Duplicate and
    the File menu advertised Place, so a shortcut that had never once worked
    was documented in a menu. Normal view and Preview view both claimed `W`.
    `the_shipped_table_has_no_clashes` now stands guard.
  - It also brought a dozen documented-but-dead chords alive at once: `F6`,
    `F8`, `Ctrl+,`, `Ctrl+Y`, `Del`, `/`, and every tool key were in the table
    and in the menus, and nothing listened for any of them.
  - **When an action may fire is stated on the action**, as a `Guard`. The old
    handler encoded it by writing the file and history chords above an early
    `return` and the object chords below it \u2014 which worked, and could not be
    read, tested or extended without re-deriving it.
  - A bare key belongs to the text whatever its action says: `T` with a caret
    live is the letter T, and the handler refuses bare chords while typing
    outright rather than trusting each guard to remember.
  - Only *changes* are stored. A file holding all sixty would freeze this
    build's defaults into it, and the next version's better chord for something
    nobody remapped would never arrive.
  - A clash is reported where it is made and **not refused**: two chords can
    share when they can never be reachable at the same moment, and the settings
    page says so rather than deciding.
- [x] Theme tokens complete; light and dark both finished \u2192 **moved to
  milestone 1.5, phase A**, with contrast asserted by a test.
- [x] Preferences dialog. *The store itself lands in milestone 1.5 phase A,
  because phase A introduces two preferences and they need somewhere to live.*
  - Six pages, applied as changed, with per-page restore \u2014 per page rather than
    everything, because somebody who wants their blur back is not asking to
    lose their units.
- [x] Autosave and crash recovery \u2192 **moved to milestone 1.5, phase A.** Data
  safety belongs with the cross-cutting rules, not eight milestones away.
  - The preferences for it were a **lying switch** until this milestone:
    `Recovery::INTERVAL` was a hardcoded thirty seconds and nothing read either
    field. They were mislabelled too \u2014 it writes a recovery copy, not the
    document \u2014 so they say what they do now, and default on. Data safety that
    has to be switched on protects the people who did not need it.
- [x] Multiple open documents. *The structure lands in milestone 1.5 phase A;
  what remains here is the tab bar and switching between them.*
  - The map has been there since milestone 1.5 and **nothing ever put a second
    thing in it**: `new_document` and `open_from_path` both called
    `replace_document`, so opening a file made the one you had unreachable.
  - An untouched blank is replaced rather than left beside real work, and
    opening a file that is already open goes to it. Two tabs of one file are
    two histories of one file, and whichever is saved last wins silently.
  - A tab with unsaved work does not close on one click.

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
