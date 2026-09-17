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

**Without a mouse.** Click the page once, or Tab to it, and `Tab` selects the
next object on the spread and `Shift+Tab` the one before, in the order the page
is read — top to bottom, then left to right. The arrows nudge what is selected
by a point, or ten with `Shift`. `Escape` lets go. The page says what is selected to a screen reader, and where
it is in the walk: "Text frame, 2 of 4".

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

**Tabs.** Press Tab in a frame and the text after it goes to the next stop.
The stops are in the inspector's Paragraph section: a position from the left
edge of the column, whether the text sits left, centred, right or on its
decimal point, and a leader character to fill the gap. A paragraph with no
stops uses one every half inch. In a table, Tab moves to the next cell instead.

**Rules.** A paragraph can carry a rule above its first line and one below
its last, from the same Paragraph section: weight, distance from the baseline,
column-wide or as wide as the text, and a colour or the text's own. It is a
paragraph property, so a heading that moves takes its rule with it.

**Keep.** In the same section: *With next* keeps a paragraph's last line in
the column that holds the next paragraph's first — what a heading wants — and
*Lines* keeps the paragraph's own lines together, all of them or a number at
each end so no line is left alone at the bottom or top of a column. A keep
that cannot be honoured, because it would leave a column empty, is let go.

**Underline and strikethrough** sit beside Bold and Italic. They are drawn
where the font says they go, in the text's colour; a character style can set
the weight, the offset and a colour of its own.

**Kerning.** Put the caret between two letters and press Alt with an arrow:
twenty thousandths of an em a step, a hundred with Shift. The Kern field in
the inspector shows the pair's value and takes a number. The font's own pair
kerning is applied underneath; this is added to it. Tracking, beside it, is
the control for a range.

**Optical kerning.** Kerning is Metrics or Optical, on a range or in a
character style. Metrics uses the pairs the font's designer set. Optical
judges every pair from the shapes of its two letters — the wedge of white in
AV, the o tucking under the T — and replaces the font's table with what it
finds; a manual kern is still added on top. Reach for it when a font kerns
little or not at all, or when two faces or a capital and a figure meet where
no table can list the pair.

**Language.** Text has a language, set in the Character section or on a
character style, and it decides two things: which hyphenation patterns break
its words — thirty-four languages are carried — and what the font is told,
for the letterforms it keeps for one country and not another. Nothing set is
English.

**Characters with no key.** Type ▸ Insert special character lists the dashes,
the spaces, the quotes and the marks; the palette finds each by name. An em
dash is Shift+Alt+-, an en dash Alt+-, a discretionary hyphen Ctrl+Shift+-
and a non-breaking space Ctrl+Alt+X. A discretionary hyphen is a break you
allow in one word, honoured whether or not the paragraph hyphenates.

**Features.** Below Case: common and discretionary ligatures on or off,
lining or old-style figures, proportional or tabular, fractions, and
stylistic sets by number. A font that lacks a feature ignores it — nothing
here can make text disappear, only fail to change it.

**Justification and hyphenation.** A justified paragraph reaches the measure
by spacing its words, then its letters, then scaling its glyphs, within the
percentages in the Justification rows — InDesign's 80 / 100 / 133 for words
by default, letters held still, glyphs at 100 / 100 / 100 — and a word that
fits only by squeezing the spaces or the glyphs to their minimum is pulled up
onto the line. Glyph scaling draws every glyph on the line the same fraction
wider or narrower; a percent or two is invisible and buys a line, more shows.
Past every limit the words take the rest rather than leave the line short. With *Break words* on, the Hyphenation rows
say how short a word may be broken, how many letters stay on each side, how
many lines in a row may end in a hyphen, and whether capitalised words may be
broken.

**Lists.** *List* in the Paragraph section makes a paragraph a bulleted or
numbered item. The marker is generated, not typed — you cannot put a caret
in it, and moving an item renumbers the list — and a tab follows it, so the
text sits at the first tab stop. *Hang the turnover* sets the indents and the
stop so wrapped lines line up under the text. Numbers count on from the item
before; a paragraph that is not an item ends the count, and *Restart at 1*
starts it again.

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
