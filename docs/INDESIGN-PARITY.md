# InDesign parity reference

A reading of one Adobe InDesign screenshot, region by region, against Tessera
as it stands on **2026-09-13** — re-read from the 2026-09-03 original after
milestones 3 to 8 landed, when every row still said what was true six
milestones earlier. A parity file that is out of date hides gaps exactly as
well as no parity file at all. **This is a reference, not a build order.** The
build order is [ROADMAP.md](ROADMAP.md); the interface direction is
[the Instrument spec](docs/superpowers/specs/2026-09-03-instrument-milestone-design.md).

Parity with InDesign is not the goal. Parity with what editorial work
*requires* is the goal, and this file exists so that nothing InDesign does is
missing by accident rather than by decision.

## How to read the columns

**Kind** is the honest cost:

| Kind | Meaning |
|---|---|
| **model** | Needs a field in `nodes.rs` — a `.tessera` format version bump and a migration test |
| **surface** | The model already carries it; only the UI is missing |
| **view** | Neither — it is view state, a preference, or a pure command over existing data |
| **—** | Recorded, unscheduled, and probably not wanted |

**Owner** is the milestone that delivered it, or must. `M1.5` is the
Instrument milestone; `M9` is Body Copy; a dash means no milestone claims it.

**Tessera today** is `✅` when the roadmap's sentence for it has been ticked,
`✗` when nothing exists, and prose when the truth is in between. A `✅` here
carries the roadmap's caveat with it: most were verified by test, and the hand
checks are still owed.

---

## ① Tools panel

InDesign shows twenty-six tools in two columns, several behind flyouts.
Tessera has twelve. The Instrument spec's D6 refuses the modal transform tools
outright, so the target is not twenty-six.

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Selection | ✅ `Tool::Select` | — | done |
| Direct Selection (anchor editing) | ✅ `Tool::DirectSelect` | — | done, M1 |
| Page tool (per-page size) | ✅ the control bar's W/H resize the current page; Properties resizes them all | — | done |
| Gap tool | ✗ | — | — |
| Content Collector / Placer | ✗ | — | — |
| Type | ✅ `Tool::Text` | — | done |
| Type on a Path | ✅ Type ▸ Type on a path…; start, end, alignment and flip in the inspector; words in the story editor; no on-curve caret or drag handles | — | done 2026-09-17 |
| Line | ✅ `Tool::Line` | — | done |
| Pen | ✅ `Tool::Pen` | — | done |
| Add / Delete Anchor Point | ✅ | — | done, M1 |
| Convert Direction Point | ✅ | — | done, M1 |
| Pencil / Smooth / Erase | ✗ — the Pen is the drawing tool | view | — |
| Rectangle, Ellipse | ✅ | — | done |
| Polygon | ✅ `Tool::Polygon` | — | done, M1 |
| Rectangle **Frame** (graphic placeholder) | ✅ `Tool::Graphic`, `FrameKind::Graphic` | — | done, M5 |
| Scissors | ✅ `Tool::Scissors` | — | done, M1 |
| Free Transform / Rotate / Scale / Shear | ✗ | — | **refused** (D6) |
| Gradient Swatch | gradients are set in the inspector; there is no drag-to-angle tool, by D6's argument | — | done, M5 |
| Gradient Feather | ✗ | model | — |
| Note | ✗ | — | — |
| Eyedropper | ✗ | view | — (recorded under M9) |
| Color Theme | ✗ | — | — |
| Measure | ✗ | view | — |
| Hand | ✅ `Tool::Hand` | — | done |
| Zoom | ✅ `Tool::Zoom` | — | done, M1 |
| Fill / stroke proxy, swap, default, none | ✅ | — | done, M1.5 C5 |
| Formatting affects container vs text | by structure: the inspector's Fill section is the container, the Type section's colour is the text | — | done |
| Apply colour / gradient / none | ✅ | — | done, M5 |
| Screen modes — Normal, Preview | ✅ | — | done, M1.5 C9 |
| Screen modes — Bleed, Slug | ✅ | — | done, M1.5 C9 |

## ② Control panel

**Refused as a surface** — see D1. The functions below survive; the strip does
not. They landed in the inspector (values), the control bar (the selection's
geometry, from the interface work) and the canvas toolbar (spatial verbs).

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Reference point proxy (9-point) | ✅ | — | done, M1.5 C2 |
| X, Y | ✅ | — | done |
| W, H | ✅ | — | done |
| Constrain-proportions chain | ✅ | — | done, M1.5 |
| Scale X %, Scale Y % | ✅ | — | done, M1.5 |
| Rotation angle | ✅ | — | done |
| **Shear angle** | a field in Properties; `[~]` in the roadmap for the decomposition it rests on | — | M1, partial |
| Rotate 90° CW / CCW | ✅ canvas toolbar and menu | — | done, M1.5 C7 |
| Flip horizontal / vertical | ✅ canvas toolbar and menu | — | done, M1.5 C7 |
| Stroke weight | ✅ | — | done, M1.5 C4 |
| Stroke style (solid, dashed) | ✅ | — | done, M1.5 C4 |
| Stroke alignment, cap, join | ✅ | — | done, M1.5 C4 |
| Stroke colour | ✅ | — | done, M1.5 C4 |
| Align & distribute | ✅ nineteen actions | — | done, M1.5 C6 |
| Corner options + radius | ✅ | — | done, M1 |
| Effects (`fx`) | ✅ drop shadow, blend modes | — | done, M5 |
| Opacity | ✅ | — | done, M5 |
| Object style dropdown | ✅ | — | done, M5 |
| Text-frame columns + gutter | ✅ | — | done, M4 |
| Text-frame inset, vertical justification | ✅ | — | done, M4 |
| Text wrap | ✅ box, shape, jump; wrap to largest area, both sides, left, right | — | done, M4; sides 2026-09-15 |
| Frame fitting options | ✅ fit and fill modes, inner transform | — | done, M5 |
| Select container / content / prev / next | content by direct-select; no prev / next command | view | — |
| Quick Apply | ✗ | — | **superseded** by the command palette (D3) |

## ③ Rulers and guides

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Horizontal and vertical rulers | ✅ | — | done, M1.5 C8 |
| Unit selection (mm, pt, px, in, picas) | ✅ | — | done, M1.5 A1 |
| Zero-point widget | ✅ | — | done, M1.5 C8 |
| Ruler guides (drag-out) | ✅ | — | done, M1.5 B3 |
| Margin guides | ✅ | — | done, M1.5 B1 |
| Column guides | ✅ | — | done, M1.5 B1 |
| Bleed and slug guides | ✅ | — | done, M1.5 B1 |
| Snapping with indicators | ✅ | — | done, M4 |
| Baseline grid | ✅ | — | done, M4 |

## ④ Canvas and pasteboard

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Pasteboard | ✅ | — | done |
| Page with shadow | ✅ | — | done |
| **Facing-page spread rendering** | ✅ | — | done, M1.5 B5 |
| Bleed rectangle | ✅ | — | done, M1.5 B1 |
| Margin rectangle | ✅ | — | done, M1.5 B1 |
| Pan and zoom | ✅ | — | done |

## ⑤ Pages panel

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Spread thumbnail grid | ✅ | — | done, M3 |
| Drag to reorder | ✅ | — | done, M3 |
| Add / delete / duplicate page | ✅ | — | done, M3 |
| Parent (master) pages section | ✅ | — | done, M3 |
| Per-page parent badge | ✅ the section marks the master the current page takes | — | done, M3 |
| Parent item override | ✅ | — | done, M3 |
| "N Pages in M Spreads" count | ✅ as "3 of 12" in the status bar | — | done, M1.5 C10 |
| Edit page size | ✅ per page from the control bar, document-wide from Properties | — | done |

## ⑥ Properties panel, "No Selection" state

This is InDesign's document inspector. Tessera's is the document-setup state
of the inspector, M1.5 B7.

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Page-size preset (A4, Letter…) | ✅ | — | done, M1.5 B7 |
| Page W / H | ✅ | — | done, M1.5 B7 |
| Orientation | ✅ | — | done, M1.5 B7 |
| Page count | ✅ | — | done, M3 |
| **Facing Pages** toggle | ✅ | — | done, M1.5 B2 |
| Margins T/B/L/R with chain | ✅ | — | done, M1.5 B7 |
| Bleed and slug | ✅ | — | done, M1.5 B7 |
| Adjust Layout | ✗ | view | — |
| Page navigation, Edit Page | ✅ status bar and pages panel | — | done |
| Rulers & Grids toggles | ✅ | — | done, M1.5 |
| Guides toggles | ✅ | — | done, M4 |
| Quick Actions (Import File…) | ✅ as File > Place | — | done, M5 |

## ⑦ Panel dock and workspace

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Collapsed icon rail | ✅ | — | done, M7 |
| Tabbed panel stacks | ✅ | — | done, M7 |
| Splitters | ✅ | — | done, M7 |
| Workspace switcher and presets | ✅ | — | done, M7 |
| Document tab bar with dirty marker | ✅ | — | done, M7 |
| Layers panel | ✅ | — | done, M3 |
| Links panel | a Links section in the inspector — status, relink, update; no panel of its own | — | done, M5 |
| Swatches panel | ✅ | — | done, M5 |
| Stroke panel | ✅ in the inspector | — | done, M1.5 |
| Paragraph / Character styles | ✅ | — | done, M2 |
| Object styles | ✅ | — | done, M5 |
| Effects panel | ✅ in the inspector | — | done, M5 |
| Text wrap panel | ✅ in the inspector | — | done, M4 |

## ⑧ Status bar

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Zoom percentage control | ✅ | — | done, M1.5 C10 |
| Page navigator | ✅ | — | done, M1.5 C10 |
| **Live preflight indicator** | ✅ | — | done, M6 |
| Status / message area | ✅ | — | done |

## ⑨ Menu bar

InDesign: File, Edit, Layout, Type, Object, Table, View, Plug-Ins, Window,
Help. Tessera: File, Edit, Layout, Type, Object, Table, View, Window, Help.

| Menu | Owner |
|---|---|
| Layout, Type, View, Window | ✅ M1.5 C12, generated from the action list |
| Table | ✅ tables landed 2026-09-12 |
| Plug-Ins | — no extension surface is planned |
| Help | ✅ M8 |

## ⑩ What the screenshot does not show

The screenshot is a workspace, and the tables above are now mostly ticks. What
the 2026-09-13 re-read found is that a screenshot cannot show the typography a
person reaches for once they start setting copy, and none of it had a row:

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Tab stops and leaders | ✅ | — | done, M9 |
| Paragraph rules above / below | ✅ | — | done, M9 |
| Keep options, widow / orphan control | ✅ | — | done, M9 |
| Bullets and numbering | ✅ | — | done, M9 |
| Underline, strikethrough | ✅ | — | done, M9 |
| OpenType features (figures, ligatures, stylistic sets) | ✅ | — | done, M9 |
| Kerning control, optical kerning | ✅ manual kern at the caret; Metrics or Optical on a range or a style | — | done, M9; optical 2026-09-17 |
| H&J parameters | ✅ word spacing, letter spacing, glyph scaling, own breaker | — | done, M9; glyph scaling 2026-09-17 |
| Auto page number, sections, section marker | ✅ | — | done, M10 |
| Text variables; running header (paragraph style) | ✅ custom text and running header | — | done, M10 |
| Footnotes, endnotes | ✅ reference, note at the column foot (splitting across columns) or gathered at the end by Layout ▸ Endnotes…, box to edit it, document footnote options | — | done, M11; split + endnotes 2026-09-17 |
| Table of contents, index | ✅ generated stories, placed and updated from Layout | — | done, M11 |
| Hyperlinks, bookmarks | ✅ URL and page links on text, PDF annotations, contents entries link; the contents headings are the PDF outline | — | done |
| Language on text; special-character insertion | ✅ | — | done, M9 |
| Typographer's quotes; glyph by code point | ✅ a preference, and a box; no panel drawing the font | — | done |
| Spell check | ✅ Hunspell dictionaries from a folder, dynamic spelling, suggestions in the box and on right-click; no bundled list | — | done; squiggles + suggestions 2026-09-15 |
| Story editor | ✅ a plain box over the story, applied as a minimal edit | — | done |
| Print dialog | ✗ — PDF export only | — | — |
| IDML import | ✅ pages, parents, threads, styles, swatches, sections, footnotes, anchored objects, tables | — | done, M12 |
| Word import | ✅ File ▸ Place a `.docx`; styles merged by name | — | done, M12 |

---

## What this exercise established

**On 2026-09-03: most of the screenshot was a model gap.** The majority of
missing elements needed a field in `nodes.rs` before any pixel could be drawn,
and each one was a format version bump with a migration test. The milestones
were priced by the model, not by the widget count, and that pricing held.

**On 2026-09-13: the screenshot is nearly closed, and it was the wrong
picture.** What remains from it is refused or unwanted. What is missing is in
section ⑩, none of which a screenshot of a workspace can show, and all of
which a person setting a document notices before they notice a missing tool.
Milestone 9 exists because this file did not have those rows.

**Three InDesign surfaces are refused rather than deferred.** The control
panel (D1), the modal transform tools (D6), and Quick Apply — the last
superseded by a command palette that does the same job without being hidden
inside the surface that created the problem.
