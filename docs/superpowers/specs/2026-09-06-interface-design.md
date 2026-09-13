# The Tessera interface — design record

**Date:** 2026-09-06
**State:** phases 1–6 in `main`. The light theme landed first and the density
preference followed it; both switch while the application is running.

A companion to the visual version of this document, which carries the mockups
and the palette swatches. This file is the part that has to survive without a
browser: what was decided, and what it was argued from.

---

## What Tessera is, in Cooper's terms

Alan Cooper's *About Face* classifies applications by **posture**. Tessera is
**sovereign**: a program a person works inside for hours at a time, full
screen, with nothing else competing for attention. That classification is not
a label — it changes the answer to almost every visual question, and it
contradicts the advice that circulates for web and mobile interfaces.

| The usual advice | Cooper, for a sovereign application |
| ---------------- | ----------------------------------- |
| Generous whitespace | Pack tighter than other kinds of application: familiarity comes from exposure, so controls can be smaller and closer |
| Colour creates hierarchy | Mute it — "big colourful controls seem garish after a couple of weeks of daily use" |
| Respect the window the user chose | Take the whole screen; default to maximised |
| Optimise for first-time clarity | Optimise for **perpetual intermediates** — neither novices nor experts |

That last row is the sharpest, and the easiest to get wrong. Designing Tessera
around somebody's first ten minutes is designing for a tenth of a percent of
the time it will be used.

## The sources, and what each settled

**Linear — [behind the latest design refresh](https://linear.app/now/behind-the-latest-design-refresh).**
Their governing line is *"don't compete for attention you haven't earned."*
Concretely: the sidebar went several notches dimmer, icons shrank and some were
removed, coloured icon backgrounds went, and sharp dividers became softer and
lower in contrast — *"structure should be felt, not seen."* They also moved the
whole neutral from cool blue-grey to a warmer grey.

→ Settled that Tessera's chrome recedes and that `RULE` and `BORDER` are two
different roles: a line that groups is quieter than a line that bounds a
control. Settled the warm neutral.

**Figma — [behind our redesign, UI3](https://www.figma.com/blog/behind-our-redesign-ui3/).**
Panels are resizable and collapsible rather than fixed, and the interface can
be hidden entirely. They put backgrounds on inputs and borders on dropdowns so
controls read as controls. The two **reversions** are the valuable part: they
tried putting size above position and put it back because it *"disrupted muscle
memory"*, and they condensed the alignment grid and reverted it because it
*"moved around too much."*

→ Settled that fields sit a step above the panel rather than being identified
by a border alone. Settled the rule that **nothing moves that was not moved**,
and that X and Y come before W and H.

**Radix — [understanding the scale](https://www.radix-ui.com/colors/docs/palette-composition/understanding-the-scale).**
Twelve steps, each with exactly one job: 1–2 grounds, 3–5 component normal /
hover / pressed, 6–8 borders subtle / interactive / focus, 9–10 solid accent
and hover, 11–12 low- and high-contrast text.

→ Adopted wholesale, replacing five ad-hoc greys that had no rule about which
went where. The role-to-step mapping is **asserted in a test**: a table in a
doc comment drifts, an assertion cannot.

**Adobe — [Spectrum 2](https://adobe.design/stories/design-for-scale/introducing-spectrum-2).**
They rebuilt around the finding that a professional tool's density and contrast
are a **preference, not a constant**, and ship controls for both.

→ The reason spacing is a token scale rather than numbers in panels, and the
reason phase 6 is worth doing rather than optional.

## Decisions that came from Tessera rather than from anyone else

**Chrome is neutral, always.** Colour is the work here. Every accent in the
interface is one desaturated blue; magenta, cyan and red belong to margins,
guides and bleed, which are the *document* speaking. The mark's red `#C3282D`
appears on the application icon and nowhere else — a red rule beside a page is
a red somebody will try to match.

This is the same argument the code already made for `PREVIEW_SURROUND`, which
is held constant across themes because perceived colour shifts with what
surrounds it. It generalises to the whole chrome.

**The pasteboard is a role, not a step.** In a dark theme the deepest ground
sits right behind a white page; in a light one the same step would be a white
surround behind white paper and a page would have no shape at all.

**And the trade underneath that.** Requiring 3:1 between paper and pasteboard
forces the pasteboard dark enough that a blue selection edge stops reading on
it — a saturated hue sits at about the luminance of a mid grey by definition,
so no single colour has high contrast against both white paper and a mid-grey
ground. A page is drawn with its own edge and its own shadow, so the fills do
not have to carry the distinction alone. The test records which side of that
trade was taken and why.

## What the contrast tests caught

Written by eye, four of these would have shipped:

- the focus ring failed 3:1 against the panel in **both** palettes;
- light labels failed 4.5:1 on a selected row;
- the light frame edge was invisible on the pasteboard;
- the light accent was invisible on the pasteboard the first fix produced.

Two further invariants now hold the scale itself: every step climbs in one
direction, and steps 1–8 are never cool.

## Phase 6, and why it was one job

A light theme and a density preference were the same piece of work, and for the
same reason: the tokens were compile-time constants — `Theme::PANEL_BG`,
`Theme::SPACE_2` and the rest — and anything that can change while the
application runs has to be read at runtime instead. A wide but mechanical
change to every use, made in one pass rather than half-made, because a scale
that is runtime in some panels and compile-time in others draws two densities
at once.

Both are done. Colours read `palette()`; the spacing scale reads `density()`
through `Theme::space_1()` and its siblings, across eighty-seven call sites.

Two things that were not in the plan came out of doing it.

**There were two spacing scales.** `SPACING_SM/MD/LG` had survived beside
`SPACE_1..4` with fifteen call sites between them — the duplication the four
named steps were introduced to end, still there. They fold together at equal
values, which leaves one regular scale: 4, 8, 12, 16, 20.

**Half the interface is not drawn by us.** egui builds its own style once and
draws every button, field and tab from it, so a density that moved only the
hand-drawn spacing would have meant "the same controls, closer together" —
which is the half of density that helps nobody. `interact_size` now reads the
scale like everything else.

What density deliberately does **not** move is the type size and the corner
radius. Scaling the text as well would make it a zoom, which is a different
control answering a different question, and one the platform already provides.

## On inspiration sites

Mobbin and Godly are worth the time. Dribbble is not, for this: it optimises
for a single still image, which rewards big type, generous space and low
density — every one of which is wrong for a sovereign application. The best
references for Tessera are InDesign, Blender and Nuke, and Cooper is the one
who explains *why* they look the way they do.
