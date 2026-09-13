# Using Tessera

For somebody who has opened it and wants to lay something out. It assumes no
knowledge of Tessera and some knowledge of what a page is.

## The first thing you see

A **New document** dialog, because a page size, a bleed and a press are
decisions a job is built on — changing them later means moving everything laid
out against the old ones.

Press Return and you get A4, facing pages, 12.7mm margins, 3mm bleed, CMYK. That
is a document a printer would accept.

The one choice worth stopping on is **Colour**:

- **CMYK** for anything going on a press. The document is proofed and separated
  through a real press profile.
- **RGB** for a screen, the web, a presentation. Colour stays as it is typed and
  nothing is proofed or separated.

You are not asked which ICC profile. There are nine, seven of which differ in
ways only a printer can explain, and print starts on **CRPC6** — premium coated
sheet-fed, the widest gamut among the coated conditions, so a document proofed
against it is not flattered by a press with less. Change it in **Layout ▸
Document setup** once you know who is printing.

**Preview** shows the page behind the dialog as you change it. A page size is
hard to picture from two numbers and easy to recognise on sight.

## The window

| | |
| --- | --- |
| Top | The menu bar, then the **control bar** — what is selected, and its position, size and angle |
| Left | The tools |
| Right | The panels, as tabs. Drag a tab to move it, or onto **New group** to split the side |
| Bottom | The status bar: what is selected, the page size, the tool, and the preflight state |

The panel you are in is named; the rest are their icons. Hover one to see which.

## Tools

Every tool has a single-key shortcut, and they are InDesign's, because that is
what a layout designer's fingers already know.

| Key | Tool | |
| --- | --- | --- |
| `V` | Select | Whole objects |
| `A` | Direct select | Anchor points on a path |
| `M` | Rectangle | |
| `L` | Ellipse | |
| `G` | Polygon | Sides and star depth are in the control bar |
| `\` | Line | |
| `P` | Pen | |
| `T` | Type | |
| `F` | Frame | A box for artwork |
| `C` | Scissors | Cuts a path where you click |
| `H` | Hand | Or hold space |
| `Z` | Zoom | Click in, Alt-click out, drag to a region |

## Text

Draw a frame with `T` and type. Double-click an existing frame with the select
tool to put a caret in it.

**Threading.** When more text arrives than fits, the frame's bottom-right corner
shows a red `+` — the *out port*. Click it, then click the frame the text should
continue into. That is the only warning you get that copy has fallen off the end
of a frame, because overset text is invisible by definition.

To break a thread: select the frames and **Object ▸ Unthread**.

**Styles** are in the Styles panel. Double-click one to edit it — the list is
what you look at every few minutes, so the two dozen properties live in a window
of their own.

## Colour

The Swatches panel holds named colours. A **spot** colour is a pot of a specific
ink and gets a plate of its own on the press; a **process** colour is mixed from
the four.

**Soft proofing** — View ▸ Soft proof — shows the document as the chosen press
will print it, which usually means duller. That is not a fault: it is the gamut
of the ink, and seeing it now is better than seeing it on paper.

## Before you send it

**Preflight** runs continuously and the status bar says what it found. Eight
checks: overset text, missing and modified links, low resolution, colour space,
objects short of the bleed, unresolved swatches, missing fonts, and whether a
press has been chosen at all. Click a problem to jump to it.

Errors mean the job comes back wrong. Warnings mean somebody should look.

**Export** — File ▸ Export PDF. Choose PDF/X-1a or PDF/X-4 if the printer asked
for one. Tessera will **refuse** to write a standard it cannot honour rather
than claim it: a printer's preflight believes the file, so a document claiming
X-1a it does not meet passes their check and fails on the press instead of in
the studio.

**Package** — File ▸ Package — collects the document, its links and a summary a
printer can read. Fonts are *listed*, not copied: a licence to set type is not a
licence to pass the font on, and the PDF carries subsetted outlines, which is
what makes the job printable.

## Things worth knowing

- **Undo is per gesture, not per frame.** Dragging forty objects is one entry.
- **Ctrl while dragging** suspends snapping.
- **Recovery copies** are written every 30 seconds while a document has unsaved
  changes, and offered back after a crash. They are not autosave: your file is
  only ever written when you save it.
- **Workspaces** (Window ▸ Workspace) remember which panels are open and how
  they are arranged — not your document, and not where you were on the page.
- **Density** (Preferences ▸ Appearance) sets how tightly the panels are
  packed: compact, standard or comfortable. It moves the spacing and the height
  of every row, and deliberately leaves the type size alone — a setting that
  scaled the text as well would be a zoom, which is a different question.

## When something is wrong

The status bar carries the last thing Tessera had to say. If an action appears
to do nothing, look there first: refusals are stated rather than silent, and
that is where they are stated.
