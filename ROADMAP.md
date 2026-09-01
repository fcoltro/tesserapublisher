# Tessera Publisher — Roadmap

A professional desktop publishing application: an InDesign-class layout tool,
free and genuinely cross-platform. Linux is a first-class target, not an
afterthought — the absence of a serious DTP application on Linux is the reason
this project exists.

**Architecture:** native Rust, egui 0.35 + eframe, Vello for the document
surface. No webview. See
[the rebuild design](docs/superpowers/specs/2026-09-01-tessera-rebuild-design.md).

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

- [ ] **Save and open never lose data.** Round-trip property tests pass on
  arbitrary generated documents, and every historical format version still
  loads through its migration chain.
- [ ] **Cross-platform build parity.** CI builds and runs the headless suite
  on Linux, Windows and macOS. No platform-specific code outside
  `apps/tessera_app/src/platform/`.
- [~] **Interactive verification.** Windows only, by choice. Linux and macOS
  are **known-unverified** and are recorded as such, never as done.
- [ ] **No unsafe code.** `unsafe_code = "forbid"` at the workspace level.
- [ ] **No silent fallbacks.** Every failure states its cause in the interface
  or in a log. The previous codebase's most instructive defect was a clip
  rectangle that resolved to zero and disabled rendering without a word.
- [ ] **Tests land with the change**, in the same commit.

---

# Milestone 0 — The Walking Skeleton

**The spine. Nothing else counts until this works.**

Every crate exists and is real. None is deep. This milestone deliberately
produces an application that does very little and *keeps every bit of it*.

### Acceptance — one sentence, performed on Windows against a release build

> Launch Tessera. A new document opens with one spread. Draw a rectangle and
> give it a fill colour. Draw a text frame, **type into it on the canvas**,
> and see the text shaped and rendered. Save the file as `.tessera`. Quit the
> application. Launch it again, open that file, and find the rectangle and the
> text exactly as they were left. Export a PDF, and open that PDF in Acrobat
> with the text selectable.

- [ ] **Step 0 — the wgpu spike.** Prove Vello can render into a texture on
  the device eframe owns, on egui 0.35. **This runs before anything else is
  built**, because a negative result changes the design (see R1 in the spec).
- [ ] **Step 1 — demolition.** Remove `src/`, `src-tauri/`, `crates/core`,
  `crates/renderer`, and all Node tooling. The old tree stays in git history
  and is consulted, not carried.
- [ ] **Step 2 — the workspace.** Nine crates and one app, dependencies
  pointing downward only, `unsafe_code = "forbid"`, CI on three platforms.
- [ ] A window opens, egui draws, the canvas pans and zooms.
- [ ] Rectangles and text frames can be drawn, selected and moved.
- [ ] Text is typed **on the canvas**, with a caret, selection, and backspace.
- [ ] `.tessera` saves and loads, with round-trip property tests.
- [ ] PDF exports with embedded, subsetted fonts and RGB colour.
- [ ] Undo and redo work across every one of the above.

**Explicitly not in M0:** docking, panels beyond one inspector, master pages,
threading, swatches, preflight, images, CMYK, print marks.

---

# Milestone 1 — The Editing Surface

Making the skeleton pleasant to use. No new file-format surface area.

### Acceptance

> Draw rectangles, ellipses, lines and free paths. Select several objects at
> once with a marquee and move them together. Rotate an object and scale it
> from any handle. Nudge with arrow keys. Copy, paste, duplicate and delete.
> Zoom to fit, zoom to selection, and pan with the spacebar. Undo any of it,
> then redo it.

- [ ] Tool state machine: select, direct-select, rectangle, ellipse, line,
  pen, text, hand, zoom.
- [ ] Marquee selection, shift-extend, and select-all.
- [ ] Transform handles: move, scale from any handle, rotate, with modifier
  constraints (proportional, from centre, angle snap).
- [ ] Clipboard, duplicate, and step-and-repeat.
- [ ] Z-order: bring forward, send backward, to front, to back.
- [ ] Grouping and ungrouping.
- [ ] Numeric transform fields with drag-to-scrub.
- [ ] Every gesture records exactly one undo entry, on completion.

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
- [ ] Facing-page spreads with correct left/right geometry.
- [ ] Master pages, applied by drag, rendered behind page content.
- [ ] Master item override, promoting one item to a local editable copy.
- [ ] Layers panel: named layers, reorder, visibility, lock.
- [ ] Document setup: page size, orientation, margins, bleed, slug.

---

# Milestone 4 — Layout Systems

### Acceptance

> Drag ruler guides and have objects snap to them, to the page edge, to the
> margins, and to each other. Set up a multi-column text frame. Turn on a
> baseline grid and lock text to it. Thread a long story through three frames
> across two pages, resize the first, and watch the text reflow through the
> chain. See the connector lines between linked frames when one is selected.

- [ ] Rulers with unit selection (mm, pt, px, in, picas).
- [ ] Ruler guides, margin guides, column guides.
- [ ] Snapping solver with a pixel-threshold lock and visible indicators.
- [ ] Baseline grid with a per-frame lock toggle.
- [ ] Multi-column text frames with gutter control.
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
- [ ] Theme tokens complete; light and dark both finished.
- [ ] Multiple open documents.
- [ ] Preferences dialog.
- [ ] Autosave and crash recovery, surfaced on relaunch.

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
