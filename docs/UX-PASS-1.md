# UX pass 1 — cursors, transform zones, text editing, hit testing

Ten reported problems, grouped into seven changes. Ordered so the build stays
green at every step.

## 1. Lucide cursors, painted rather than requested  (icons.rs, cursor.rs)

`egui::CursorIcon` is a fixed vocabulary mapped onto OS cursors. Windows has no
native `Grab`, so winit substitutes `IDC_SIZEALL` — the move cross. That is the
whole reason rotation shows a move cursor today.

We already carry Lucide as parsed geometry (`icons.rs`). Paint the cursor into
the canvas overlay and set `CursorIcon::None` over the viewport.

- New icons: `Grab`, `Rotate`, `Move`, `Scale`, `TextCursor`, `Crosshair`.
- `paint` gains a rotation, so ONE scale icon (`move-horizontal`) serves all
  eight handles by pointing along the handle's real on-screen normal — which
  is what makes it correct on a rotated frame.
- Each icon declares a hotspot in grid units, so the cursor's point lands on
  the pointer rather than its bounding box's corner.
- Painted twice — a dark casing under a light stroke — so it stays legible on
  both the pasteboard and a white page.

## 2. Transform zones that do not overlap  (viewport.rs)

- Scale: within `HANDLE_GRAB_PX` of a handle.
- Rotate: **outside** the frame's rotated box and within `ROTATE_RING_PX` of a
  corner. Measured in the frame's local space so "outside" is well defined.
  Never inside — which is what the ring affordance means.
- Move: inside the shape.

Scale beats rotate where they touch. A frame too small for both keeps scale.

## 3. Group box tracks the group  (document.rs, viewport.rs, transform.rs)

A group already HAS `bounds` and `rotation`; nothing maintains them.

- `presented()` reads the group's own box, not a recomputed union.
- `origins_of()` includes the group itself, so a gesture updates it.
- `scale_origins` works in the group's local space: un-rotate a child's centre
  about the group centre, scale, re-rotate. Children keep their own rotation.
- `rotate_origins` turns the group's own rotation too, so the box swings
  rigidly instead of breathing.

## 4. Reference mark  (viewport.rs)

Replace the ring-and-crosshair with InDesign's mark: a thin 1px x, ~4px arms.

## 5. Hit testing follows the shape  (document.rs)

`hits()` is a bounding-box test for every kind, so a pen curve claims its whole
box. Make it kind-aware:

- Rectangle / Text — the rect, as now.
- Ellipse — the normalised ellipse equation.
- Path — map frame-local geometry onto bounds (the same `fit_to_bounds` the
  renderer uses, moved down into `tessera_document` so there is one copy),
  then winding for a filled closed path, and distance-to-curve for a stroke.
- Tolerance passed in from the viewport as screen pixels / zoom, so a hairline
  stays clickable at every zoom.

## 6. Text frames are always outlined  (viewport.rs)

Every text frame strokes a thin non-printing edge, as InDesign does. Overlay
only, so it can never reach a PDF.

## 7. Real text editing  (shape.rs, caret.rs, viewport.rs)

The caret is drawn at the frame's left edge at full height because nothing maps
a byte offset to a position.

- `PositionedGlyph` gains `cluster` (byte offset into the story);
  `ShapedLine` gains `y_top`, `height` and `text_range`. Additive — only
  `shape.rs` constructs them.
- New `tessera_text::caret`: offset -> caret rect, and point -> offset.
- The viewport stops early-returning during editing: clicking inside sets the
  caret, dragging selects, clicking outside commits and leaves.
- Selection is drawn as highlight rects behind the glyphs.

## 8. Antialiasing  (viewport.rs, scene.rs)

Two causes, neither of them the antialiaser.

**A sub-pixel blit.** Vello renders into a texture sized `(rect.width() * ppp)
as u32` — truncated — which egui then paints across the *unsnapped* logical
rect. When the canvas lands on a fractional physical pixel, which is routine at
125% and 150% display scaling, every texel is sampled halfway between two of
them and the whole document is bilinearly smeared. A 45-degree edge hides that
in its two-axis gradient; a near-vertical one has nowhere to hide it. The
canvas rect is now snapped to whole physical pixels and the texture sized from
the snapped box, with `round` rather than a truncating cast.

**No hairline floor.** A one-point rule at 25% zoom is a quarter of a device
pixel. Vello draws that honestly — a quarter of the coverage — and it reads as
a line that fades, breaks up along its length, and flickers as the view moves.
Worst when it runs nearly straight down a column of pixels, because every pixel
in the run makes the same wrong decision. Strokes are now clamped to at least
one device pixel of width, on screen only: the PDF writer does not go through
`build_scene`, so an export keeps the width the document specifies.

The sRGB path was checked and is **correct** — egui's shader samples user
textures as gamma-encoded (`tex_gamma`), which is what Vello writes into
`Rgba8Unorm`. It was left alone.

`AaConfig::Area` was also left alone. It is Vello's analytic coverage path and
the right choice here; the artefacts were upstream of it.

## 9. The inspector agrees with the handles  (command.rs)

Falling out of change 3: with a group's box authoritative, `SetBounds` and
`SetRotation` writing straight into the group frame would stretch the box away
from the artwork. Both now route a group through the same `rotate_origins` and
`scale_origins` a drag uses, so a number typed into the inspector and a handle
dragged on canvas mean the same thing.

---

# Round 2 — from the first run

## 10. The zone is decided by where the press landed  (viewport.rs)

The reported bug: the cursor says scale, the click rotates.

egui does not report a drag until the pointer has travelled past a threshold —
several pixels. The gesture read `interact_pointer_pos()` at `drag_started`,
which is the position *after* that travel, not where the button went down.
Press on a scale handle, drift six pixels outward, and the point being
classified is now outside the box and inside the rotate ring. The cursor never
lied; it was asked a different question, at a different place.

Every zone decision now starts from `press_origin()`, and the cursor freezes on
the press origin too while the button is held, so it cannot change out from
under a press it has already promised something to.

The pen's hotspot was read off the wrong corner — `(2.3, 21.7)` instead of
`(2.3, 2.3)`. Lucide's `pen-tool` points up and to the LEFT. The old value was
still inside the icon's bounding box, so a bounds check would not have caught
it; the test added instead asserts a pointed icon's hotspot lies within 1.5
units of its own ink.

## 11. The marquee catches content  (document.rs)

`Document::frames_touching` replaces an `area.intersects(f.bounds)` filter. A
band catches a frame when it contains part of the outline, when the outline
crosses one of its edges — tested exactly against the curve, not a flattened
approximation — or, for a filled shape only, when the band lies wholly inside
it. Same rule clicking follows, including that an unfilled outline does not own
its empty middle.

It now walks top-level order rather than paint order, so a band over part of a
group takes the group. Paint order only ever yields children, which contradicted
what a click on the same object did.

## 12. Cursors are one weight, inverted  (cursor.rs, theme.rs)

The dark casing under the light stroke read as a heavier, blobbier icon than
the toolbar's. Gone: one stroke, Lucide's own weight, in one of two colours
chosen by what is behind it.

**This is a two-tone choice, not a true inversion.** egui composites with
premultiplied alpha and offers no difference blend, so the cursor cannot invert
per pixel. The canvas has exactly two backgrounds — the white page and the dark
pasteboard — and the colour is picked from which one the pointer is over. The
known gap: over a dark-filled object *on* the page, the cursor is dark on dark.
If that bites, the next step is sampling the resolved fill under the pointer.

## 13. The cursor yields to menus  (viewport.rs)

The painted cursor lives in the canvas layer, so an open menu drew over it —
"it stays behind". `show_cursor` now requires `response.hovered()`, which egui
documents as false when another layer is on top. Over a menu the platform
cursor is left alone, so it looks the way it looks over the toolbar. Still
painted mid-drag, when the pointer may legitimately be anywhere.

## 14. Grips work while text is being edited  (viewport.rs)

The scale-and-rotate half of a select drag became `transform_gesture`, shared by
`select_gesture` and `editing_input`. A text frame keeps its grips while its
caret is live, and resizing reshapes the text as it goes, because the shaper is
asked again every frame.

Two guards this needed: a press on a grip must not be read as a click in or out
of the text — a corner grip sits ON the frame's edge, which the inside test
reads as outside, so grabbing a handle would have ended the edit — and a click
that began on a grip must not fall through and reselect whatever the handle is
sitting over.
