# Milestone 1.5 — The Instrument

**Status:** design approved 2026-09-03. Implementation plan not yet written.

A companion to
[the rebuild design](2026-09-01-tessera-rebuild-design.md), which this
document does not replace. That spec settled the architecture. This one
settles the interface.

---

## 1. Why this milestone exists

Two facts, both established by reading the code rather than by argument.

**Capability has shipped that a user cannot reach.** `Stroke` carries
alignment, caps, joins, miter limit, dashes and dash offset — built, tested,
and exposed nowhere. `Color` models CMYK and spot; nothing can create either.
`Layer` carries `visible` and `locked`; no surface toggles them. The inspector
in `crates/tessera_ui/src/view/panels.rs` renders position, size, rotation,
text and fill, and stops.

**The roadmap's rule permits fixing exactly this.** ROADMAP.md defers the
workspace to milestone 7 because building chrome before the application could
keep a user's work was the previous implementation's defining mistake. That
rule forbids chrome *ahead of* capability. It does not forbid — it demands —
the missing surface for capability already finished.

So this milestone adds no capability to the document model. Its defining
constraint is stated in §7.

## 2. What prompted it

A screenshot of Adobe InDesign, read region by region against Tessera as it
stands. The full element-by-element comparison lives in
[`docs/INDESIGN-PARITY.md`](../../INDESIGN-PARITY.md), which assigns every
item to an owning milestone or records it as unscheduled.

The comparison produced one structural finding worth repeating here: **most of
what the screenshot shows is a model gap, not a UI gap.** Object opacity,
corner radius, graphic-frame-versus-shape, parent pages, margins, bleed, slug,
object styles, text-frame columns, swatches, gradients, text wrap and image
links are absent from `nodes.rs`. Each is a `.tessera` format version bump
carrying a migration test. That, and not the number of widgets, is what the
remaining milestones cost.

## 3. Position

Tessera takes from InDesign the things thirty years of editorial practice
proved right: the reference point as the anchor of every transform, the
pasteboard as a place to keep work in progress, stroke alignment as a property
distinct from weight, and the discipline that a layout tool answers to a
printer rather than to a screen.

It refuses the parts that are age rather than wisdom. InDesign's interface was
laid out for a 2003 display and a user who would be trained. Its control panel
is a horizontal strip, distant from the object it edits, whose contents shuffle
position with the selection — so its own controls have no stable address and
must be hunted for each time. Its tools panel holds twenty-six tools behind
flyouts that hide state. It offers three separate routes to a rotation: a
modal tool, a handle, and a field. Its answer to its own discoverability
problem, Quick Apply, is a lightning-bolt button buried in the strip that
caused it.

None of that is carried forward.

## 4. Design decisions

Each states what was chosen, why, and what was rejected — the form used by the
rebuild spec.

### D1 — No control panel. One inspector, with a stable address.

Every value lives in the right-hand inspector, in sections whose order never
changes: **Transform, Fill, Stroke, Text, Frame**. A section that does not
apply to the selection is hidden, and the sections that remain **do not move
up to fill the gap**. Position is learnable only if it is constant, and a
control that relocates by context is one the hand cannot find without the eye.

*Rejected: an InDesign-style contextual strip above the canvas.* It fails on
two counts. Distance — the fields sit at the top of the window while the object
sits in the middle, so every numeric edit is a long round trip. And width — the
strip in the reference screenshot occupies roughly 1900 logical pixels, which
does not fit the window sizes Tessera must support, and the overflow chevrons
Adobe uses to cope are themselves hidden state.

### D2 — Verbs on the canvas, nouns in the rail.

A small contextual toolbar appears beside the selection carrying **only actions
whose effect is spatial**: align, distribute, flip horizontal, flip vertical,
rotate 90° clockwise, rotate 90° anticlockwise. At most six buttons. It follows
the selection and vanishes with it.

The split is principled rather than aesthetic. Values — a width, a stroke
weight, an angle — are read and compared, so they belong in a stable list.
Actions whose result is a change of position or orientation are judged by
looking at the object, so the control belongs next to the object. Nothing
appears in both places, so the two surfaces cannot disagree.

*Rejected: putting these actions in the inspector too.* Two routes to one
outcome is the duplication that makes InDesign's transform story confusing.
*Also rejected: no canvas toolbar at all.* Alignment is the most frequent
operation in page layout and the one most punished by a long pointer journey.

### D3 — A command palette over the existing `Command` enum.

`Ctrl`/`Cmd`+`K` opens a fuzzy-filtered list of every command, showing each
one's keyboard shortcut beside it.

This is close to free: `command.rs` already routes every mutation through one
`Command` enum, so the palette is a filter over a list that exists, and it
teaches shortcuts as a side effect of being used. It also discharges an
obligation the roadmap places on milestone 7 — a shortcut for every common
command — by making every command reachable whether or not it has one.

*Rejected: deep menus and tool flyouts as the discovery mechanism.* Hidden
state, and it does not scale past about forty commands.

### D4 — The reference point is drawn where it acts.

The nine-point proxy sits at the head of the Transform section at a size the
pointer can actually hit, and **the chosen anchor is also painted on the
selection itself** on the canvas, as a small mark.

InDesign's proxy is a grid of targets a few pixels across, in a corner of the
screen, whose state silently changes the meaning of every field beside it and
every scale gesture on the canvas. Putting the mark on the object makes the
state visible in the place the user is already looking, which is the only place
a mode can be safely displayed.

### D5 — Units are parsed, not moded.

Every numeric field accepts a unit suffix. Typing `12mm` into a field showing
points converts and stores. Fields display the document's preferred unit, and
the ruler's unit selector changes that preference everywhere at once.

*Rejected: a units mode that reinterprets bare numbers.* A modal unit makes the
same keystroke mean different things at different times, which is the defining
property of a mode error.

### D6 — One Select tool. No modal transform tools.

Move, scale, rotate and shear are all reached from the Select tool through
handle zones that do not overlap — the arrangement `UX-PASS-1.md` already
settled — or numerically from the inspector. InDesign's separate Rotate, Scale,
Shear and Free Transform tools are not built.

Every mode is a chance to act in the wrong one. Four modes that each do what
one tool's handles already do is four chances bought for nothing.

### D7 — Icons stay geometry, and are parsed once.

`icons.rs` holds Lucide paths as SVG text, parses them with `kurbo`, and paints
through `egui::Painter` — no image files, no icon font, crisp at any DPI,
re-tinted by the theme. That decision stands and the icon set grows from 15 to
roughly 60 to cover the tools, inspector, palette and status bar.

One change: the paths are parsed on **every paint** today. At sixty icons on
screen that is sixty `BezPath` allocations per frame. They become a
parse-once cache keyed by `Icon`, built on first use.

*Rejected: an SVG runtime or an icon font.* Both add a dependency to replace
something that is already a few hundred bytes of text and one existing parser.

### D8 — Light and dark, and a pasteboard that does not lie.

Both themes ship, with text contrast meeting WCAG AA. In **Preview** screen
mode the pasteboard and page surround are painted a fixed neutral grey in both
themes.

This is a colour-judgement requirement, not a preference. Perceived colour
shifts with its surround, so a designer choosing an ink against a dark chrome
in one theme and a light chrome in the other would be choosing two different
inks. The surround is therefore held constant at the moment the user is judging
colour.

## 5. The surfaces

**Inspector** — Transform (reference proxy, X, Y, W, H with a constrain chain,
scale X/Y as percentages, rotation, shear), Fill, Stroke (weight, style,
alignment, cap, join, colour), Text, Frame. Numeric fields keep the existing
drag-to-scrub behaviour and gain unit parsing.

**Canvas toolbar** — the six spatial verbs of D2.

**Command palette** — D3.

**Rulers** — horizontal and vertical, with a unit selector and a zero-point
widget at their intersection. See R2: they measure, and cannot yet yield
guides.

**Screen modes** — Normal and Preview, on `W`. Preview hides handles, frame
edges, rulers and the canvas toolbar. Bleed and Slug modes need document setup
and belong to milestone 3.

**Status bar** — zoom control, page navigator, and the existing message area.

**Menus** — Layout, Type, View and Window join File, Edit and Object, carrying
only commands that exist. A menu entry for an unbuilt feature is a lie the
previous codebase told often.

**Fill and stroke proxy** — with swap on `X`, defaults on `D`, none on `/`.

## 6. Performance

Stated as invariants to be tested, not as aspirations.

- **The Vello scene is rebuilt only when `(document.revision(), view)`
  changes.** Both terms already exist. A repaint that changes neither reuses
  the scene.
- **No continuous repaint.** eframe is reactive; nothing in this milestone may
  call `request_repaint` on a timer. A tool that redraws sixty times a second
  while idle costs battery on every laptop that runs it.
- **Icon paths parse once** (D7).
- **Text shaping stays behind `tessera_layout::cache`.** Inspector edits must
  not invalidate a layout that did not change.
- **Budget: 16.7 ms per interactive frame** at 60 Hz, measured on the drag of a
  selection across a page holding 500 frames.

## 7. The constraint that defines the milestone

**Revised 2026-09-03, after a sequencing review.** The original constraint was
*no format version bump*, on the reasoning that a milestone which touched the
model would drift into building chrome ahead of capability.

That constraint was wrong, and reviewing the ordering is what showed it.
Deferring the page geometry pushed a foundation behind everything that stands
on it — rulers, screen modes, align-to-page, `TrimBox` and `BleedBox`,
preflight's out-of-bleed rule — and PDF export already ships without a bleed
box. It also forced R2 below: a ruler that could not yield a guide, purely to
avoid a bump.

The constraint is now **one bump, batched, in a phase of its own**. A format
version costs a migration test; page size, margins, bleed, slug, facing pages
and guides delivered together cost one, and delivered separately cost five.

The milestone therefore runs in three ordered phases:

**Phase A — Foundations.** No UI, no format surface. Units, a preferences
store, affine decomposition, anchor resolution, the open-document container,
the command invariant, a performance harness, theme tokens, the icon cache,
autosave. Everything here is small, pure and depended upon.

**Phase B — The Page.** The single format bump: page geometry, facing pages,
guides as data, `ColorRef`, a spread that renders as a spread, migration tests,
the document inspector, and the PDF boxes.

**Phase C — The Instrument.** The interface described in §4 and §5, built on
both.

The honesty test survives in a better form: **an item belongs to phase C only
if phases A and B have already made it buildable.** Chrome ahead of capability
is prevented by the ordering rather than by a prohibition.

## 8. Acceptance

> Select a rectangle. Set its position and size numerically with the reference
> point on its centre, and watch it scale about that point — with the anchor
> mark visible on the object as it does. Type `12mm` into a field reading
> points and see it convert. Give the rectangle a 3 pt dashed stroke, aligned
> inside, with round caps, and see it on the canvas. Swap fill and stroke with
> one key. Select three objects and align their left edges from the toolbar
> beside them. Shear one. Press `Ctrl`+`K`, type "flip", and flip it. Read its
> position off a ruler in millimetres, switch the ruler to picas, and watch
> every field in the application follow. Press `W` and see the handles, frame
> edges and rulers go, leaving the page on a neutral surround; press it again
> and get them back. Switch to the light theme and read every label.

Performed by hand on Windows against a release build. Linux and macOS build
and pass the headless suite in CI, and are recorded as unverified.

## 9. Testing

Headless, landing in the same commit as the code, in the pattern
`crates/tessera_ui/tests/milestone_0.rs` established:

- Reference-point resolution across all nine anchors, for scale, rotate and
  flip.
- Affine decomposition: a property test that a transform built from a known
  scale, shear, rotation and translation decomposes back to them, and
  recomposes to the same matrix.
- Unit parsing and display round-trips for mm, pt, px, in and picas.
- Align and distribute over generated frame sets, including the degenerate
  cases of one object and of coincident objects.
- Command palette: every `Command` variant is reachable by at least one query.
- Theme contrast: every foreground-on-background pair in `theme.rs` meets
  WCAG AA, asserted in a test rather than checked by eye.
- Scene rebuild is skipped when neither revision nor view changed.
- `crates/tessera_ui/tests/milestone_1_5.rs` drives the acceptance sentence
  end to end.

## 10. Risks

**R1 — the affine decomposition.** `Transform::rotation_degrees()` assumes no
shear, and `selection.rs`, `transform.rs`, `panels.rs` and `scene.rs` rely on
it to place handles. Introducing shear makes that assumption false.
*Mitigation:* add `decompose() -> (scale, shear, rotation, translation)`
beside it, migrate callers one at a time, and leave `rotation_degrees()`
delegating until the last one has moved.

**R2 — rulers without guides. ~~Accepted~~ Dissolved 2026-09-03.** This risk
existed only because §7 originally forbade a format bump, which left the ruler
unable to yield a guide. Once the sequencing review moved page geometry into
phase B, the bump was being paid anyway and guides rode along at no extra
cost. Recorded rather than deleted, because a risk that disappears when a
constraint is questioned is worth remembering as a pattern: **the risk was
downstream of a self-imposed rule, not of the problem.**

**R3 — two surfaces for transforms.** D2 splits verbs from nouns, but a user
who does not perceive the split will look in the wrong place. *Mitigation:*
the split is absolute, with nothing appearing in both, and the command palette
reaches everything regardless.

**R4 — canvas toolbar occlusion.** A toolbar beside the selection can cover
the object beside it. *Mitigation:* it is placed on the side with more free
space, and suppressed entirely in Preview mode.

## 11. Out of scope

Deferred with their owning milestone, not forgotten:

Corner radius, object opacity, effects, object styles, gradients, swatches
panel, text wrap, frame fitting, text-frame columns and inset, image links,
parent pages, layers panel, ruler guides, snapping, dockable panels, workspace
switcher, document tabs, Bleed and Slug screen modes.

Recorded as unscheduled, and unlikely: the Gap tool, Content Collector and
Placer, the Note tool, the Color Theme tool, and separate modal Rotate, Scale
and Shear tools.
