# The LayoutPro design, read off the prototype

The reference is a Figma Make prototype, `Desktop Publishing Software UI`
(file key `BH1E6oaVHcr6yCY7SjipFJ`), built by the author of this project and
titled **LayoutPro** in its own chrome.

## Why this file exists

The prototype cannot be reached from a build machine. Figma Make files are
behind a login; the Figma MCP can *list* their source (`App.tsx`, `index.css`,
four PNGs) but its own server answers `does not support resources` when asked
for the contents, and `get_screenshot`, `get_metadata` and `get_variable_defs`
all state outright that they do not support `/make/` files. The design was read
by driving a logged-in browser and screenshotting the rendered prototype.

So this file is the record. **It is a transcription, not a link**: anything not
written down here is not available to the next person, and a value below that
disagrees with the prototype should be corrected here rather than only in code.

Values marked *(read)* were read from the prototype's own interface — its hex
fields and its generated summary. Values marked *(measured)* were estimated off
a screenshot and are approximations to be tuned by eye.

## The shape of the window

Seven bands, outside in:

1. **Menu bar** — app mark (rounded square, gradient fill), the word
   `LayoutPro`, then `File Edit Layout Type Object View Window Help`. On the
   right: undo/redo, zoom out `−` `85%` `+` zoom in, two view-mode buttons, the
   theme toggle, `Share`, and a settings gear.
2. **A gradient hairline** immediately under the menu bar, running the full
   width — purple through blue to cyan. This is the single strongest identity
   mark in the design and costs one rectangle.
3. **Control bar** — the measurements strip: `X Y W H`, rotation, shear, the
   align and distribute buttons, then `Fill` and `Stroke` swatches and
   `Opacity`. Context-sensitive to the selection.
4. **Left icon rail** — one column of icons, no labels, roughly 28pt wide,
   loosely grouped: pointer, then view/search, then drawing tools, then shapes,
   then type and image, then transform and eyedropper. A fill/stroke swatch pair
   sits at the very bottom.
5. **Layers / Pages panel** — a two-tab panel. Rows carry a type icon, a name,
   and eye and lock buttons at the right. `+ New Layer` pinned at the foot.
6. **Canvas** — rulers along the top and left with numbered ticks, a fine grid,
   and the page centred on it. Selection is a dashed rectangle with square
   handles at corners and edge midpoints.
7. **Right properties panel** — `Design | Prototype | Inspect` across the top,
   then sections under small-caps headings.
8. **Status bar** — monospaced, dim. Left: `Cover · 1 object selected  |
   A4 · 210 × 297 mm  |  Tool: cursor`. Right: `Preflight: ● No errors  |
   Zoom 85%`.

## The right panel's sections

In order, each under a small-caps, letter-spaced, dim heading:

- **TYPOGRAPHY** — family dropdown; then `Size` / `Leading` side by side, then
  `Tracking` / `Kerning`; then a row of B / I / U and four alignment buttons.
- **FILL & STROKE** — two rows, each a colour chip, a hex field, and a numeric
  field (`100 %` for fill opacity, `1 pt` for stroke width).
- **SWATCHES** — a grid of small rounded chips, ten to a row, with a dashed
  "add" chip at the end.
- **GRADIENT** — a wide preview bar, then `Type` and `Angle` fields.
- **EFFECTS** — a list of named effects, each with a pill toggle on the right.
  `Drop Shadow` on; `Inner Glow`, `Bevel & Emboss`, `Satin` off and dimmed.
- **OBJECT STYLE** — one field naming the applied style.
- **TRANSFORM** — `X` / `Y`, `W` / `H`, `Rotation` / `Shear`, in pairs.

The two-column pair is the panel's whole layout rule: a label above a field,
two to a row, and the section heading spanning both.

## Colour

### What the prototype does

| Role | Dark | Light |
| --- | --- | --- |
| Window ground | `#080b14`, a blue-biased near-black | `#eef0f5`, a cool neutral grey |
| Panels | white-tinted glass | white-tinted glass |
| Toggle | violet, moon | amber, sun |

Both themes run off the same variables and cross-fade in 250ms.

### What Tessera does instead, and why

**Tessera's greys are neutral, not cool.** This is a deliberate departure,
made after seeing the cool palette running, and it is the one place the
implementation argues with the design.

A tinted interface makes a claim about the paper beside it. Cool greys make
warm stock look yellow and warm greys make it look blue, and which way the
chrome leans is not a judgement this software gets to make on somebody else's
job — it is the same reason ISO 3664 asks for a neutral surround around a
proof. In a mockup the tint is styling; in a window somebody colour-corrects
in for eight hours it is a thumb on the scale.

So the accent carries all the colour there is, and
`the_greys_are_neutral_in_both_palettes` holds the line.

**There is no lit ground behind the chrome, and panels are solid.** The
prototype's glass sits over a decorative wash. Tessera had one — three
coloured lights in `view::ambient` — and it is gone for the reason above: a
violet gradient beside a proof is a claim about the proof. `lights()` still
exists, returns nothing, and is the single place a light would go back.

With nothing worth seeing through them, panels default to solid. The glass
machinery is intact and still selectable; it now frosts a flat ground.

## What this means for Tessera

Most of it already exists. Tessera has a menu bar, an icon rail, dockable
panels, a canvas, a status bar and the glass. The gaps are:

- The gradient hairline under the menu bar. Nothing like it exists.
- The control bar: `X Y W H`, rotation, shear, align, fill/stroke, opacity as a
  strip under the menu, rather than only in the properties panel.
- Ruler tick numbers and the canvas grid.
- The status bar's content and its monospace.
- The right panel's section rhythm — small-caps headings and the two-column
  label-above-field pair — applied consistently.
- **A light palette that has been looked at.** The dark one is Tessera's own and
  is close; the light one is currently untested, and this design is not
  optional about having both.

The gradient hairline under the menu bar is kept, and is now the only coloured
decoration in the window. It stays because it is the design's own identity mark
rather than a tint applied to a working surface: it sits on the menu bar, not
beside the page.

## What will not carry over

The prototype is React and CSS; Tessera is egui, drawn immediately. The visual
system ports; some of the mechanism cannot.

- **CSS `backdrop-filter`.** Tessera generates its blur instead of filtering
  what is behind it, because the ground behind a panel is procedurally ours.
  See `GLASS.md`. The look is reachable; the technique is not the same one.
- **Arbitrary nested drop shadows.** egui draws shadows on frames, not on
  everything.
- **Layout transitions.** The 250ms theme cross-fade is a colour interpolation
  and is reachable; a transition on *layout* is not, because an immediate-mode
  frame has no previous layout to animate from.

Where a substitution is made it belongs in this list, said plainly, rather than
quietly approximated.
