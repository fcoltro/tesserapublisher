# InDesign parity reference

A reading of one Adobe InDesign screenshot, region by region, against Tessera
as it stands on 2026-09-03. **This is a reference, not a build order.** The
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

**Owner** is the milestone that must deliver it. `M1.5` is the Instrument
milestone; a dash means no milestone claims it.

---

## ① Tools panel

InDesign shows twenty-six tools in two columns, several behind flyouts.
Tessera has seven. The Instrument spec's D6 refuses the modal transform tools
outright, so the target is not twenty-six.

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Selection | ✅ `Tool::Select` | — | done |
| Direct Selection (anchor editing) | ✗ | surface | M1 |
| Page tool (per-page size) | ✗ | model | M3 |
| Gap tool | ✗ | — | — |
| Content Collector / Placer | ✗ | — | — |
| Type | ✅ `Tool::Text` | — | done |
| Type on a Path | ✗ | model | M4 |
| Line | ✅ `Tool::Line` | — | done |
| Pen | ✅ `Tool::Pen` | — | done |
| Add / Delete Anchor Point | ✗ | view | M1 |
| Convert Direction Point | ✗ | view | M1 |
| Pencil / Smooth / Erase | ✗ | view | M1 |
| Rectangle, Ellipse | ✅ | — | done |
| Polygon | ✗ | view | M1 |
| Rectangle **Frame** (graphic placeholder) | ✗ | model | M5 |
| Scissors | ✗ | view | M1 |
| Free Transform / Rotate / Scale / Shear | ✗ | — | **refused** (D6) |
| Gradient Swatch | ✗ | model | M5 |
| Gradient Feather | ✗ | model | M5 |
| Note | ✗ | — | — |
| Eyedropper | ✗ | view | M5 |
| Color Theme | ✗ | — | — |
| Measure | ✗ | view | M4 |
| Hand | ✅ `Tool::Hand` | — | done |
| Zoom | ✗ (wheel only) | view | M1 |
| Fill / stroke proxy, swap, default, none | ✗ | surface | **M1.5** |
| Formatting affects container vs text | ✗ | view | M2 |
| Apply colour / gradient / none | ✗ | model | M5 |
| Screen modes — Normal, Preview | ✗ | view | **M1.5** |
| Screen modes — Bleed, Slug | ✗ | model | M3 |

## ② Control panel

**Refused as a surface** — see D1. The functions below survive; the strip does
not. They land in the inspector (values) or the canvas toolbar (spatial verbs).

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Reference point proxy (9-point) | ✗ | view | **M1.5** |
| X, Y | ✅ inspector | — | done |
| W, H | ✅ inspector | — | done |
| Constrain-proportions chain | ✗ | view | **M1.5** |
| Scale X %, Scale Y % | ✗ | view | **M1.5** |
| Rotation angle | ✅ inspector | — | done |
| **Shear angle** | ✗ | view | **M1.5** |
| Rotate 90° CW / CCW | ✗ | view | **M1.5** |
| Flip horizontal / vertical | only by dragging a handle through itself — no command, no button | view | **M1.5** |
| Stroke weight | ✗ | **surface** | **M1.5** |
| Stroke style (solid, dashed) | ✗ | **surface** | **M1.5** |
| Stroke alignment, cap, join | ✗ | **surface** | **M1.5** |
| Stroke colour | ✗ | **surface** | **M1.5** |
| Align & distribute | ✗ | view | **M1.5** |
| Corner options + radius | ✗ | model | M1 |
| Effects (`fx`) | ✗ | model | M5 |
| Opacity | ✗ | model | M5 |
| Object style dropdown | ✗ | model | M5 |
| Text-frame columns + gutter | ✗ | model | M4 |
| Text-frame inset, vertical justification | ✗ | model | M4 |
| Text wrap | ✗ | model | M4 |
| Frame fitting options | ✗ | model | M5 |
| Select container / content / prev / next | ✗ | view | M5 |
| Quick Apply | ✗ | — | **superseded** by the command palette (D3) |

## ③ Rulers and guides

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Horizontal and vertical rulers | ✗ | view | **M1.5** |
| Unit selection (mm, pt, px, in, picas) | ✗ | view | **M1.5** |
| Zero-point widget | ✗ | view | **M1.5** |
| Ruler guides (drag-out) | ✗ | model | M4 |
| Margin guides | ✗ | model | M3 |
| Column guides | ✗ | model | M3 |
| Bleed and slug guides | ✗ | model | M3 |
| Snapping with indicators | ✗ | view | M4 |
| Baseline grid | ✗ | model | M4 |

## ④ Canvas and pasteboard

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Pasteboard | ✅ canvas background | — | done |
| Page with shadow | ✅ page drawn | — | done |
| **Facing-page spread rendering** | ✗ — `build_scene` takes one page | model | M3 |
| Bleed rectangle | ✗ | model | M3 |
| Margin rectangle | ✗ | model | M3 |
| Pan and zoom | ✅ | — | done |

## ⑤ Pages panel

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Spread thumbnail grid | ✗ | view | M3 |
| Drag to reorder | ✗ | view | M3 |
| Add / delete / duplicate page | ✗ | view | M3 |
| Parent (master) pages section | ✗ | model | M3 |
| Per-page parent badge | ✗ | model | M3 |
| Parent item override | ✗ | model | M3 |
| "N Pages in M Spreads" count | ✗ | view | M3 |
| Edit page size | ✗ | model | M3 |

## ⑥ Properties panel, "No Selection" state

This is InDesign's document inspector. Tessera has no document-setup surface
anywhere; `Page` carries `bounds` and `layers` and nothing else.

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Page-size preset (A4, Letter…) | ✗ | model | M3 |
| Page W / H | ✗ | model | M3 |
| Orientation | ✗ | model | M3 |
| Page count | ✗ | view | M3 |
| **Facing Pages** toggle | ✗ | model | M3 |
| Margins T/B/L/R with chain | ✗ | model | M3 |
| Bleed and slug | ✗ | model | M3 |
| Adjust Layout | ✗ | view | M3 |
| Page navigation, Edit Page | ✗ | view | M3 |
| Rulers & Grids toggles | ✗ | view | **M1.5** |
| Guides toggles | ✗ | view | M4 |
| Quick Actions (Import File…) | ✗ | model | M5 |

## ⑦ Panel dock and workspace

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Collapsed icon rail | ✗ | view | M7 |
| Tabbed panel stacks | ✗ | view | M7 |
| Splitters | ✗ | view | M7 |
| Workspace switcher and presets | ✗ | view | M7 |
| Document tab bar with dirty marker | ✗ | view | M7 |
| Layers panel | ✗ — `Layer` has `visible`/`locked` unreached | **surface** | M3 |
| Links panel | ✗ | model | M5 |
| Swatches panel | ✗ | model | M5 |
| Stroke panel | ✗ | surface | **M1.5** (in the inspector) |
| Paragraph / Character styles | ✗ | model | M2 |
| Object styles | ✗ | model | M5 |
| Effects panel | ✗ | model | M5 |
| Text wrap panel | ✗ | model | M4 |

## ⑧ Status bar

| Element | Tessera today | Kind | Owner |
|---|---|---|---|
| Zoom percentage control | ✗ | view | **M1.5** |
| Page navigator | ✗ | view | **M1.5** |
| **Live preflight indicator** | ✗ | view | M6 |
| Status / message area | ✅ | — | done |

## ⑨ Menu bar

InDesign: File, Edit, Layout, Type, Object, Table, View, Plug-Ins, Window,
Help. Tessera: File, Edit, Object.

| Menu | Owner |
|---|---|
| Layout, Type, View, Window | **M1.5** — carrying only commands that exist |
| Table | — no table model is planned before M6 |
| Plug-Ins | — no extension surface is planned |
| Help | M8 |

---

## What this exercise established

**Most of the screenshot is a model gap.** Counting the rows above: the
majority of missing elements need a field in `nodes.rs` before any pixel can be
drawn, and each one is a format version bump with a migration test. The
remaining milestones are priced by the model, not by the widget count.

**Capability has shipped with no surface.** The rows marked **surface** —
stroke weight, style, alignment, cap, join and colour; layer visibility and
lock — are finished, tested code that no user can reach. Milestone 1.5 exists
to close that specific gap, and its constraint is that it may close no other.

**Three InDesign surfaces are refused rather than deferred.** The control
panel (D1), the modal transform tools (D6), and Quick Apply — the last
superseded by a command palette that does the same job without being hidden
inside the surface that created the problem.
