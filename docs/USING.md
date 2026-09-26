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
| `I` | Eyedropper | Click an object to pick up its fill, stroke, effects, corners and type; click others to give it to them; Alt-click to pick up afresh |

With the select tool over nothing but a page's right or bottom edge — or the
corner where they meet — the cursor turns, and dragging makes that page
another size; one undo puts it back.
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

The top of that window shows the style being edited in its own face, size,
colour and spacing, and updates as you change it. Beneath it, a blue dot marks
a property the style states. A hollow ring marks one it inherits: the value is
greyed, with the style it comes from — "12 pt, from [Basic Paragraph]". Change
a greyed value and the style states it. Click the ring to state it as it is,
or click a dot to go back to inheriting. The header shows what the style is
based on (click a parent to edit it) and has **Apply to selection**. The
sidebar counts what the style states on each page, **Reset page** clears one
page, and General lists everything, with **Reset to base**. Character colour
can name one of the document's swatches, so editing the swatch recolours every
style that uses it.

General also counts where the style is used. The arrows beside the count
select each use on the page in turn, and the window stays open. A character
style is previewed where the document first uses it, among the words around
it. It is set in a paragraph style that you choose under the preview
("Shown in"); by default that is the paragraph style of that first use. What
the character style leaves alone shows as that paragraph has it: "13 pt, as in
Body".

An object style is previewed as a box in a column of text. It is drawn with
the style's fill, stroke, opacity and shadow, and the text runs round it as the
style's wrap says. The box is drawn on the first object that follows the style
(or a plain rectangle when none does), so what the style leaves alone shows as
that object has it. Fill and stroke colours come from the same swatch tiles as
text, with [None] for no fill. The stroke's weight, alignment, ends, joins and
dash pattern, the shadow's offset, blur and colour, the blend mode, and the
wrap's distances and sides are all set there. General counts the objects that
follow the style and those with changes of their own, and the arrows select
each one in turn.

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

**Glyphs.** Window ▸ Glyphs opens a panel in the rail drawing every character
the face at the caret has, in that face; choose another family at the top to
browse it. Search by name — *arrow*, *em dash*, *euro*, *section* — by the
character itself pasted in, or by code (*U+2026*, *2026*), and narrow the grid
to letters, numbers, punctuation, symbols, arrows, maths or currency. Point at
a character to see it large with its name and code; click it to put it at the
caret, or, with no caret in text, to choose it. *Copy* copies it to paste
anywhere; the bookmark keeps it among the **favourites** above the grid, with
the characters used lately — both kept between runs. Right-click a character
for the same. The magnifiers draw the grid larger or smaller. Type ▸ Insert
special character ▸ Glyph by code point… is the same thing for a character
whose number you know.

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

**Type on a path.** Draw a path with the pen or the line tool, select it, and
choose Type ▸ Type on a path…. The path stays a path — stroke it, move it,
reshape its anchors — and now carries a story, opened in the story editor for
you to write. The *Type on a path* section of the inspector says where along
the path the text starts and ends, as percentages of its length; whether the
letters stand on the line (*Baseline*), have it run through them (*Centre*),
hang from it (*Ascender*) or stand clear above it (*Descender*); and *Flip*,
which runs the text the other way on the other side, turning text inside a
circle into text outside it. Words that do not fit are overset, and preflight
reports them. The words are edited in the story editor rather than on the
curve.

**Notes.** Type ▸ Insert footnote puts a reference at the caret and opens a
box for the note; the note is set at the foot of the column that cites it,
under a short rule, and a note too long for the room continues at the foot
of the next column. Type ▸ Footnote options… says what the numbers count in,
where they restart, the space and the rule — and *Placement*. Set to *End of
the document*, nothing is set at the foot: Layout ▸ Endnotes… gathers every
story's notes into one story, numbered as their references are, places it
on the current page, and updates it afterwards from the same command.

**Index.** Type ▸ Insert index entry files the caret's place under a topic;
"Type: Serif" nests Serif under Type. *Reaches* says how far the mention
runs — this page, the next few paragraphs, or to the end of the story — so a
subject discussed for a chapter is indexed "12–15". Layout ▸ Index… places
the index and updates it.

**Book.** Window ▸ Book opens the Book panel. *New book…* makes a book file —
a list of documents that are one publication — or *Open book…* one, and the
books opened lately are listed until one is. *Add chapters…* fills it, and
every change to the list is saved as it is made.

Each chapter's row says the pages it runs to as the book numbers them, how
many pages it has, whether it is open in a tab (a ring) and unsaved (a dot),
and, in red, when it is missing; drag a row to reorder, double-click one to
open it, and choose one for *Open*, *Locate…* for a missing one, or *Remove*
— which takes it out of the book and leaves the file. When the chapters' own
page numbers are not the ones the book gives them, the panel says so above
the list.

With *Number pages on from chapter to chapter* ticked, *Number pages* gives
each chapter's first page the number after the chapter before's last, saving
chapters that are not open and changing open ones undoably; it waits while a
chapter is missing, since a book numbered around a gap is numbered wrong.
*Update contents* rebuilds the open chapter's table of contents from every
chapter's headings, *Check chapters* preflights every chapter and puts each
one's count of problems on its row, and *Export PDF…* writes the whole book as
one file. The ••• menu opens another book, shows this one's folder, or
closes it.

## Pages

The **Pages** panel shows every page as it is, drawn by the same renderer as
the canvas. Facing pages sit either side of one spine down the panel, as they
sit either side of the fold, so page one stands alone on the right; pages that
do not face are laid out in rows across it. The three page icons beside the
count set how large they are drawn. Each page carries the letter of the parent
it is built on in its corner, and a triangle over it where a numbering section
starts; the numbers of the pages the canvas shows are on a pill.

A click chooses a page and turns to it. Shift-click chooses every page from
the last one clicked, and Ctrl-click (Cmd on a Mac) adds or removes one. What
the panel does, it does to the chosen pages — or, with none chosen, to the
page you are on — and the foot of the panel says which: *Page 4*, *Pages
4–6*. There, **+** inserts a page after them, built on the same parent as the
page it follows; the copy button duplicates them, the copies together after
the last of them; the bin deletes them, and is refused when they are every
page. Drag a page to move it; drag one of several chosen pages and they move
together. Right-click a page for the same, its parent, *Remove local
overrides*, and *Numbering & section options…*.

**Parent pages** are listed above, each drawn small with how many pages are
built on it. A click puts it on the chosen pages; drag it onto any page to put
it there; double-click to open it on the canvas and edit it. Right-click one
to rename it in place, apply it to every page, or delete it. *[None]* takes
the parent off.

## Layers

The **Layers** panel lists the layers top to bottom, as they stack, and under
each one what it holds on the spread in view — front first, each by its kind
and what it says, with the page it is on. The triangle folds a layer's objects
away. Click a layer to draw on it; double-click its name to rename it; drag it
to restack; click its colour to change the colour its selections are drawn in.
Its eye hides it and its padlock locks it — drawn but not touched. Alt-click
either for every *other* layer: the eye shows that layer alone, and again
shows them all; the padlock locks the others.

Click an object's row to select it; Shift-click selects the run from the last
one clicked, Ctrl-click one more. Point at a row for its own eye and padlock:
an object can be hidden or locked on its own, as with **Object ▸ Lock and
hide** — *Lock* (Ctrl+L), *Hide* (Ctrl+3), *Unlock all on spread*
(Ctrl+Alt+L), *Show all on spread* (Ctrl+Alt+3). A hidden object is not drawn,
printed or wrapped round; a locked one cannot be selected on the page or in
the panel. Drag an object's row in front of or behind another, or onto another
layer's row; drag one of several selected objects and they all go.

The small square at the end of a layer's row is filled in its colour when the
selection is on that layer: drag it onto another layer to move the selection
there, or click it to select everything the layer holds on this spread.
Right-click a layer for *Hide others*, *Lock others*, its colour, and *Delete
layer…*; right-click an object for *Hide*, *Lock* and *Move to layer*. The foot
of the panel adds a layer, moves the selection to the active one, and deletes
it — asking first when it holds anything.

## Colour

The Swatches panel holds named colours. A **spot** colour is a pot of a specific
ink and gets a plate of its own on the press; a **process** colour is mixed from
the four.

Each row shows the colour, the space it is written in — CMYK, RGB, Lab, or
*Tint* — and how many places use it: objects, text, styles and tints, not only
fills and strokes. A click chooses a swatch and *Apply* puts it on the
selection's **fill**, **stroke** or **text**, as the switch above the list
says; the swatch the selection already wears carries a dot. Choosing and
applying are two steps on purpose, so looking through the list with an object
selected does not recolour it. Past eight swatches a filter narrows the list
by name.

Double-click a swatch — or *Edit swatch…* — for the **Swatch window**. A
colour is edited in its own numbers: a CMYK swatch as inks, with a slider for
each whose track shows what the colour becomes along it, and it stays CMYK.
*Mode* converts it to RGB or Lab — the same colour in other numbers, through
the document's press when one is chosen — and *Ink* makes it a spot or a
process colour. Beside the colour on screen is the colour as that press will
print it, with a warning when the press cannot reach it; the row of tints
under it makes any of them a tint swatch with a click. A tint swatch is a
share of another and follows it. *In this document* counts the uses, and
*Next* and *Previous* go to each object and each stretch of text in turn; the
styles and tints listed there open with a click. The name is edited in place
at the top.

Deleting a swatch nothing uses deletes it. Deleting one in use asks what its
uses should become — each keeps the colour it has now, or takes another
swatch — rather than leaving them naming a colour that no longer exists.

**Soft proofing** — View ▸ Soft proof — shows the document as the chosen press
will print it, which usually means duller. That is not a fault: it is the gamut
of the ink, and seeing it now is better than seeing it on paper.

## Links

The **Links** panel lists every file the document shows, once each however
many frames show it, by name: its picture, its pixels and the resolution it
prints at, and at the right the page it is on — or the parent page it is
drawn on — and how many times it is placed. A picture's resolution is its
pixels over the size it is drawn, frame scaling included, and a file placed
twice is as good as its smaller use.

The counts across the top say what is wrong: files **missing** from where they
were, **modified** on disk since they were placed, and short of the
**resolution** Preferences asks for. Click one to list only those files; click
it again for all of them. *Update all* reads every changed file again, and
*Find missing…* asks for a folder and relinks every missing file found in it,
or up to four folders inside it, by name — the usual repair for a job whose
artwork was moved. Each is one step to undo.

Click a row to go to its first frame and see the file in full below the list:
its picture, kind, size, date, folder, and every place it is used, each a
click away. *Relink…* points it at another file and every frame showing it
follows; *Update* reads a changed file again; *Show in folder* (*Show in
Explorer*, *Reveal in Finder*) and *Open* hand it to the system. Right-click a
row for the same, and *Copy path*.

## Before you send it

**Preflight** runs continuously and the status bar says what it found: click
it for the **Preflight** panel. Nine checks: overset text, missing and
modified links, low resolution, colour space, objects short of the bleed,
colours the document does not define — in text and styles as well as fills —
missing fonts, and whether a press has been chosen at all.

Errors mean the job comes back wrong. Warnings mean somebody should look. The
top of the panel says which it comes to — *Not ready to print*, *Ready, with
warnings*, or *No problems found* — and when there are both, the counts under
it list only the errors or only the warnings.

Problems are listed under the check that found them, one row per object with
the page it is on. Click a row, or walk the list with *Next* and *Previous*,
to select the object and bring it into view — on its parent page when that is
where it is. The row gone to opens out with what fixes it: *Fit frame to
text* for overset text, *Relink…* or *Update* for a file, *Choose a press…*,
and *Replace with…* for a missing font or an undefined colour, which changes
it everywhere it is named. *Find missing…* and *Update all* fix every file at
once. Each fix is one step to undo.

*Checks* lists the checks with a switch for each, and the lowest resolution
artwork may print at; a check switched off is not run, and the panel and the
status bar say how many are off. *Check again* reads the linked files from
disk again; everything else is checked as the document changes.

**Print** — File ▸ Print…, Ctrl+P. Choose all pages or a range. The pages are
written as a PDF and handed to the system's print path — on Windows,
whatever prints PDFs shows its own dialog for the printer and the paper; on
macOS and Linux the file goes to the default printer. For a particular press,
export the PDF instead.

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
