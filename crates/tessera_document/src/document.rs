//! The document: arenas of nodes, addressed by typed key.

use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use tessera_geometry::{DocPoint, DocRect, Transform};

use crate::ids::{FrameId, LayerId, MasterId, PageId, SpreadId, StoryId};
use crate::masters::Master;
use crate::nodes::{DocumentSetup, Frame, FrameKind, Guide, Layer, Page, PageSide, Spread};
use tessera_text::story::{
    CharacterFormat, CharacterStyle, CharacterStyleId, ParagraphFormat, ParagraphStyle,
    ParagraphStyleId, Story, Styles, TextStyle,
};

/// Stories are addressed by id and live at the document level, so a threaded
/// story flows through many frames while existing exactly once.
pub type StoryMap = slotmap::SlotMap<StoryId, Story>;

/// Where a frame should move within its layer's paint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZMove {
    Forward,
    Backward,
    ToFront,
    ToBack,
}

/// US Letter in points — the default new-document size.
/// The clear space between one spread's bleed and the next's.
///
/// A gap between the *pages* would not do: a page's bleed and slug stand
/// outside its trim, and two spreads set 36 points apart with 10mm of bleed
/// each would have their bleed boxes overlapping by twenty. What has to be
/// separated is the outermost thing drawn, not the page.
const SPREAD_GAP: f64 = 36.0;

const DEFAULT_PAGE: DocRect = DocRect {
    x: 0.0,
    y: 0.0,
    width: 612.0,
    height: 792.0,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub frames: SlotMap<FrameId, Frame>,
    pub layers: SlotMap<LayerId, Layer>,
    pub pages: SlotMap<PageId, Page>,
    pub spreads: SlotMap<SpreadId, Spread>,
    /// Text content, referenced by `FrameKind::Text`. **Part of the document,
    /// so it is saved with it** — text held anywhere else would vanish on
    /// save, which is precisely the class of bug this rebuild exists to fix.
    pub stories: StoryMap,
    /// Spread paint and navigation order.
    pub spread_order: Vec<SpreadId>,
    /// Layer stacking order, back to front. The last entry paints on top.
    ///
    /// Document-wide: one layer spans every page. `serde(default)` because a
    /// document written before version 8 kept its layers on its pages, and the
    /// migration is what fills this in.
    #[serde(default)]
    pub layer_order: Vec<LayerId>,
    /// Named parent spreads, in the order the panel lists them.
    ///
    /// A master's spread is deliberately **absent from `spread_order`**: it is
    /// not in the reading order, is never numbered, and never reflows with the
    /// document. Everything else about it is an ordinary spread.
    #[serde(default)]
    pub masters: SlotMap<MasterId, Master>,
    #[serde(default)]
    pub master_order: Vec<MasterId>,
    /// Which local frame stands in for which master item.
    ///
    /// Keyed by the **local** frame, because that is what a page holds and
    /// what a lookup starts from. An overridden item stops being the master's
    /// — it is an ordinary frame, editable and movable — but the document
    /// remembers where it came from, so the master's copy can be suppressed on
    /// that page and "remove overrides" can find its way back.
    ///
    /// A map on the document rather than a field on the frame: an override is
    /// a relationship between two frames, and a relationship belongs to
    /// neither of its ends.
    ///
    /// A `SecondaryMap` rather than a `HashMap`, and the round-trip test is
    /// what said so: JSON object keys must be strings, and a `FrameId` is not
    /// one. A secondary map is keyed by the same arena key the frames are and
    /// serialises as a list, which is also what it is.
    #[serde(default)]
    pub overrides: slotmap::SecondaryMap<FrameId, FrameId>,

    /// The layer new objects go onto.
    ///
    /// Saved with the document, as InDesign saves it — which layer you were
    /// working on is part of where you left off.
    #[serde(default)]
    pub active_layer: Option<LayerId>,

    /// Named character styles.
    #[serde(default)]
    pub character_styles: SlotMap<CharacterStyleId, CharacterStyle>,
    /// Named paragraph styles.
    #[serde(default)]
    pub paragraph_styles: SlotMap<ParagraphStyleId, ParagraphStyle>,
    /// The floor of the text cascade.
    #[serde(default)]
    pub text_default: TextStyle,

    /// Margins, bleed, slug, and whether pages face each other.
    ///
    /// `serde(default)` so a document written before page setup existed loads
    /// with none of it — which is the truth about that document, rather than
    /// a default it never chose.
    #[serde(default)]
    pub setup: DocumentSetup,
    /// Bumped on every mutation. The renderer rebuilds its scene only when
    /// this moves, so panning the camera does not rebuild anything.
    ///
    /// Not serialized: a document loaded from disk starts fresh at zero, and
    /// a revision counter carried across sessions would mean nothing.
    #[serde(skip)]
    revision: u64,
}

impl Document {
    pub fn new() -> Self {
        let mut doc = Self {
            frames: SlotMap::with_key(),
            layers: SlotMap::with_key(),
            pages: SlotMap::with_key(),
            spreads: SlotMap::with_key(),
            stories: StoryMap::with_key(),
            spread_order: Vec::new(),
            layer_order: Vec::new(),
            active_layer: None,
            masters: SlotMap::with_key(),
            master_order: Vec::new(),
            overrides: slotmap::SecondaryMap::new(),
            character_styles: SlotMap::with_key(),
            paragraph_styles: SlotMap::with_key(),
            text_default: TextStyle::default(),
            setup: DocumentSetup {
                // A **new** document faces its pages, as InDesign's New
                // Document dialog does — a layout tool is for books and
                // magazines before it is for handbills.
                //
                // Set here rather than in `DocumentSetup::default()`, and the
                // difference matters: that default is also what a file written
                // before this field existed loads as, and turning those into
                // facing-page documents would be inventing a decision their
                // author never made. `Default` means "absent"; this means "what
                // a new document chooses".
                facing_pages: true,
                ..DocumentSetup::default()
            },
            revision: 0,
        };

        let layer = doc.layers.insert(Layer::named("Layer 1"));
        doc.layer_order.push(layer);
        doc.active_layer = Some(layer);
        let page = doc.pages.insert(Page::at(DEFAULT_PAGE));
        let spread = doc.spreads.insert(Spread {
            pages: vec![page],
            guides: Vec::new(),
        });
        doc.spread_order.push(spread);

        // Through the flow once, so page one lands where a recto belongs. The
        // page was inserted at the origin above; only `reflow_spreads` knows
        // which side of the fold it is meant to be on.
        doc.reflow_spreads();

        doc
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn spread_ids(&self) -> impl Iterator<Item = SpreadId> + '_ {
        self.spread_order.iter().copied()
    }

    pub fn page_ids(&self) -> impl Iterator<Item = PageId> + '_ {
        self.spread_ids()
            .filter_map(|s| self.spreads.get(s))
            .flat_map(|s| s.pages.iter().copied())
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Every layer, bottom to top.
    pub fn layer_ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.layer_order.iter().copied()
    }

    /// The layer new frames go onto: the active one, else the bottom one.
    ///
    /// **Not "the layer of the page you drew on"** — there is no such thing any
    /// more. A new object joins the layer being worked on wherever it is drawn,
    /// and which page it is on follows from where it landed.
    pub fn default_layer(&self) -> Option<LayerId> {
        self.active_layer
            .filter(|l| self.layers.contains_key(*l))
            .or_else(|| self.layer_order.first().copied())
    }

    /// Choose the layer new objects go onto.
    pub fn set_active_layer(&mut self, id: LayerId) {
        if self.active_layer == Some(id) || !self.layers.contains_key(id) {
            return;
        }
        self.active_layer = Some(id);
        self.revision += 1;
    }

    /// Add a layer above the others and make it the active one.
    ///
    /// Above, because that is where a new layer is wanted: you add one to put
    /// something in front of what is already there.
    pub fn add_layer(&mut self, name: impl Into<String>) -> LayerId {
        let id = self.layers.insert(Layer::named(name));
        self.layer_order.push(id);
        self.active_layer = Some(id);
        self.revision += 1;
        id
    }

    /// A name no existing layer has: "Layer 1", "Layer 2", and so on.
    ///
    /// Counted from the number of layers rather than kept as a running total,
    /// then advanced past any collision — a document that has had layers
    /// deleted must not offer a name one of the survivors already uses.
    pub fn unused_layer_name(&self) -> String {
        let taken: Vec<&str> = self
            .layer_ids()
            .filter_map(|l| self.layers.get(l))
            .map(|l| l.name.as_str())
            .collect();
        let mut n = self.layer_order.len() + 1;
        loop {
            let name = format!("Layer {n}");
            if !taken.contains(&name.as_str()) {
                return name;
            }
            n += 1;
        }
    }

    /// Remove a layer and everything on it.
    ///
    /// **The last layer is refused**, for the same reason the last page is: a
    /// document with nowhere to put an object cannot be drawn in, and the way
    /// back would be undo alone. Refused by the document rather than by the
    /// caller, so no caller has to remember not to ask.
    pub fn remove_layer(&mut self, id: LayerId) -> bool {
        if self.layer_order.len() <= 1 || !self.layers.contains_key(id) {
            return false;
        }

        let frames = self
            .layers
            .get(id)
            .map(|l| l.frames.clone())
            .unwrap_or_default();
        for frame in frames {
            self.remove_frame(frame);
        }
        self.layers.remove(id);
        self.layer_order.retain(|l| *l != id);

        if self.active_layer == Some(id) {
            self.active_layer = self.layer_order.first().copied();
        }
        self.revision += 1;
        true
    }

    /// Move a layer to another depth. Positions are into `layer_order`.
    pub fn move_layer(&mut self, from: usize, to: usize) {
        if from >= self.layer_order.len() || to >= self.layer_order.len() || from == to {
            return;
        }
        let layer = self.layer_order.remove(from);
        self.layer_order.insert(to, layer);
        self.revision += 1;
    }

    /// Hand frames to another layer, keeping them where they are on the page.
    ///
    /// Moving between layers changes what is in front of what; it never moves
    /// anything, which is why a frame's page is untouched by this.
    pub fn move_frames_to_layer(&mut self, frames: &[FrameId], to: LayerId) {
        if !self.layers.contains_key(to) {
            return;
        }
        let mut moved = false;
        for frame in frames {
            let Some(from) = self.layer_of_frame(*frame) else {
                continue;
            };
            if from == to {
                continue;
            }
            if let Some(layer) = self.layers.get_mut(from) {
                layer.frames.retain(|f| f != frame);
            }
            if let Some(layer) = self.layers.get_mut(to) {
                layer.frames.push(*frame);
            }
            moved = true;
        }
        if moved {
            self.revision += 1;
        }
    }

    /// Which layer holds this frame.
    pub fn layer_of_frame(&self, frame: FrameId) -> Option<LayerId> {
        self.layer_ids().find(|l| {
            self.layers
                .get(*l)
                .is_some_and(|layer| layer.frames.contains(&frame))
        })
    }

    /// Which page a document-space point falls on.
    ///
    /// The page containing it, or the nearest page when it is out on the
    /// pasteboard — somewhere is better than nowhere, and "nowhere" was how a
    /// frame beside the page lost its spread and its clipping with it.
    pub fn page_holding(&self, at: DocPoint) -> Option<PageId> {
        // **Every** page, master pages included. A frame drawn on a master
        // has to belong to that master page; asking only the reading order
        // would hand it to whichever document page happened to be nearest,
        // and a master's contents would appear on page one.
        let all = || self.pages.keys();

        let on = all().find(|id| {
            self.pages
                .get(*id)
                .is_some_and(|page| page.bounds.contains(at))
        });
        on.or_else(|| {
            all().min_by(|a, b| {
                let distance = |id: &PageId| {
                    self.pages.get(*id).map_or(f64::MAX, |p| {
                        let c = p.bounds.center();
                        (c.x - at.x).powi(2) + (c.y - at.y).powi(2)
                    })
                };
                distance(a)
                    .partial_cmp(&distance(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        })
    }

    /// Which page a frame is on, by where its centre sits.
    ///
    /// **Derived, never stored.** Storing it is what made moving a frame across
    /// the fold a bookkeeping problem, and what made getting that bookkeeping
    /// wrong invisible until something had been clipped away.
    pub fn page_of_frame(&self, frame: FrameId) -> Option<PageId> {
        let frame = self.frames.get(frame)?;
        self.page_holding(frame.transform.apply(frame.bounds.center()))
    }

    /// The top-level frames whose centres land on this page, in paint order.
    pub fn frames_on_page(&self, page: PageId) -> Vec<FrameId> {
        self.layer_ids()
            .filter_map(|l| self.layers.get(l))
            .flat_map(|l| l.frames.iter().copied())
            .filter(|f| self.page_of_frame(*f) == Some(page))
            .collect()
    }

    /// The pages of a spread, in reading order.
    pub fn pages_of(&self, spread: SpreadId) -> Vec<PageId> {
        self.spreads
            .get(spread)
            .map(|s| s.pages.clone())
            .unwrap_or_default()
    }

    /// Which spread holds this page, master spreads included.
    ///
    /// The reading order first, because that is the common case and the answer
    /// a page number depends on; then the masters, so a frame on a master page
    /// still has a sheet to be clipped to.
    pub fn spread_of(&self, page: PageId) -> Option<SpreadId> {
        self.spread_order
            .iter()
            .copied()
            .find(|s| self.pages_of(*s).contains(&page))
            .or_else(|| {
                self.master_ids()
                    .filter_map(|m| self.masters.get(m))
                    .map(|m| m.spread)
                    .find(|s| self.pages_of(*s).contains(&page))
            })
    }

    /// Which side of its spread this page sits on.
    ///
    /// `Single` wherever there is no spine to be inside of: pages that do not
    /// face, and a spread holding only one page.
    pub fn page_side(&self, page: PageId) -> PageSide {
        if !self.setup.facing_pages {
            return PageSide::Single;
        }
        let Some(spread) = self.spread_of(page) else {
            return PageSide::Single;
        };
        let pages = self.pages_of(spread);
        if pages.len() < 2 {
            return PageSide::Single;
        }
        match pages.iter().position(|p| *p == page) {
            Some(0) => PageSide::Left,
            Some(_) => PageSide::Right,
            None => PageSide::Single,
        }
    }

    /// Add a page to a spread, sized like the one before it.
    pub fn add_page_to(&mut self, spread: SpreadId) -> PageId {
        let bounds = self
            .pages_of(spread)
            .last()
            .and_then(|p| self.pages.get(*p))
            .map_or_else(|| self.first_page_bounds(), |p| p.bounds);

        let page = self.pages.insert(Page::at(bounds));
        if let Some(s) = self.spreads.get_mut(spread) {
            s.pages.push(page);
        }
        self.revision += 1;
        page
    }

    pub fn guides_of(&self, spread: SpreadId) -> &[Guide] {
        self.spreads
            .get(spread)
            .map_or(&[], |s| s.guides.as_slice())
    }

    pub fn add_guide(&mut self, spread: SpreadId, guide: Guide) {
        if let Some(s) = self.spreads.get_mut(spread) {
            s.guides.push(guide);
            self.revision += 1;
        }
    }

    pub fn remove_guide(&mut self, spread: SpreadId, index: usize) -> Option<Guide> {
        let s = self.spreads.get_mut(spread)?;
        if index >= s.guides.len() {
            return None;
        }
        let removed = s.guides.remove(index);
        self.revision += 1;
        Some(removed)
    }

    /// The type area: the page inset by its margins.
    ///
    /// Which physical edge the inside margin falls on depends on the page's
    /// side, which is the whole reason [`Margins`] is not left-and-right.
    pub fn margin_rect(&self, page: PageId) -> Option<DocRect> {
        let bounds = self.pages.get(page)?.bounds;
        let m = self.setup.margins;
        let (left, right) = match self.page_side(page) {
            // A verso's spine is on its right.
            PageSide::Left => (m.outside, m.inside),
            PageSide::Right | PageSide::Single => (m.inside, m.outside),
        };
        Some(inset_each(bounds, m.top, m.bottom, left, right))
    }

    /// The page plus its bleed.
    ///
    /// **No bleed at the fold.** A spread is imposed as one sheet, so there is
    /// no trim between two facing pages and nothing to bleed past: the ink
    /// simply continues across. Only a page that stands alone — the first, or
    /// a last one with no partner — bleeds on all four sides.
    pub fn bleed_rect(&self, page: PageId) -> Option<DocRect> {
        let b = self.setup.bleed;
        let bounds = self.pages.get(page)?.bounds;
        let (left, right) = self.outer_edges(page, b.left, b.right);
        Some(outset_each(bounds, b.top, b.bottom, left, right))
    }

    /// The left and right amounts a page actually gets, given its side.
    ///
    /// Shared by bleed and slug, which have the same reason to stop at a fold.
    fn outer_edges(&self, page: PageId, left: f64, right: f64) -> (f64, f64) {
        match self.page_side(page) {
            // A verso's fold is on its right, a recto's on its left.
            PageSide::Left => (left, 0.0),
            PageSide::Right => (0.0, right),
            PageSide::Single => (left, right),
        }
    }

    /// The page plus its slug.
    ///
    /// Measured from the trim rather than from the bleed, so that a slug
    /// narrower than the bleed does not read as a negative one. On a press
    /// sheet the two are independent distances from the trim.
    pub fn slug_rect(&self, page: PageId) -> Option<DocRect> {
        let s = self.setup.slug;
        let bounds = self.pages.get(page)?.bounds;
        // Stops at a fold for the same reason the bleed does: there is no trim
        // there to carry marks past.
        let (left, right) = self.outer_edges(page, s.left, s.right);
        Some(outset_each(bounds, s.top, s.bottom, left, right))
    }

    /// The area a spread owns: its pages, everything that bleeds off them, and
    /// half the gap to its neighbours.
    ///
    /// A frame may hang off a page — that is what a pasteboard is for — but it
    /// may not reach into the *next* spread, which is a different sheet of
    /// paper. Content that appeared to run from one spread onto another was
    /// reading as though it had flowed there.
    pub fn spread_area(&self, spread: SpreadId) -> Option<DocRect> {
        let pages = self.pages_of(spread);
        let first = self.pages.get(*pages.first()?)?.bounds;
        let last = self.pages.get(*pages.last()?)?.bounds;

        let clearance = SPREAD_GAP / 2.0;
        let outward = self
            .setup
            .bleed
            .left
            .max(self.setup.bleed.right)
            .max(self.setup.slug.left)
            .max(self.setup.slug.right)
            .max(clearance);
        let down = self.vertical_clearance() + clearance;

        Some(DocRect {
            x: first.x - outward,
            y: first.y - down,
            width: (last.x + last.width) - first.x + outward * 2.0,
            height: first.height + down * 2.0,
        })
    }

    /// Which spread a frame is drawn on: the spread of the page it sits on.
    pub fn spread_of_frame(&self, frame: FrameId) -> Option<SpreadId> {
        self.spread_of(self.page_of_frame(frame)?)
    }

    /// Whether `at` falls on any page, as opposed to the pasteboard.
    ///
    /// Not `first_page_bounds().contains(..)`, which is what the cursor used
    /// to ask and was right only while there was one page. Over the second
    /// spread it answered "pasteboard" and the cursor turned white on a white
    /// page.
    pub fn on_a_page(&self, at: DocPoint) -> bool {
        self.page_ids()
            .filter_map(|id| self.pages.get(id))
            .any(|page| page.bounds.contains(at))
    }

    pub fn first_page_bounds(&self) -> DocRect {
        self.page_ids()
            .next()
            .and_then(|p| self.pages.get(p))
            .map_or(DEFAULT_PAGE, |p| p.bounds)
    }

    /// Mark the document changed.
    ///
    /// For the few edits made through a `&mut` field rather than a method of
    /// their own. The resolve cache is keyed on the revision, so an edit that
    /// forgets to move it is invisible until something else does — which is
    /// exactly the bug the page setup once had.
    pub fn touch(&mut self) {
        self.revision += 1;
    }

    /// Replace the page setup.
    ///
    /// A setter rather than a public field write, because **the revision has
    /// to move**. The resolved document carries every page's margin, bleed
    /// and slug rectangles, and the resolve cache is keyed on the revision —
    /// so a setup written straight into the field left the canvas showing the
    /// old margins until something else happened to bump the counter. It
    /// looked like the margins only updating when you moved an object,
    /// because that is exactly what it was.
    pub fn set_setup(&mut self, setup: DocumentSetup) {
        self.setup = setup;
        // Bleed and slug decide how far apart the spreads have to sit, so
        // changing them moves the pages. Without this, raising the bleed left
        // every spread where it was and the boxes grew into one another.
        self.reflow_spreads();
    }

    /// Resize every page. Per-page sizes are milestone 3.
    pub fn set_page_size(&mut self, width: f64, height: f64) {
        let ids: Vec<_> = self.pages.keys().collect();
        for id in ids {
            if let Some(page) = self.pages.get_mut(id) {
                page.bounds.width = width;
                page.bounds.height = height;
            }
        }
        // A taller page needs the spread below it to move down, and a wider
        // one moves its own facing partner across.
        self.reflow_spreads();
    }

    /// Put every spread where it belongs on the pasteboard.
    ///
    /// `Page.bounds` is in document space and everything downstream reads it —
    /// the rulers, align-to-page, guides, the PDF's `TrimBox`. So the bounds
    /// stay the truth and this recomputes them after anything structural.
    /// Deriving them at resolve time instead would mean every consumer
    /// learning the rule.
    ///
    /// A spread's pages run left to right; spreads stack downward. InDesign
    /// runs spreads across instead, but a wheel scrolls down and the
    /// pasteboard is the thing being navigated.
    pub fn reflow_spreads(&mut self) {
        // Every page is the size of the first, which is what `set_page_size`
        // already assumes: per-page sizes are a later milestone.
        let (width, height) = self
            .page_ids()
            .next()
            .and_then(|id| self.pages.get(id))
            .map_or((DEFAULT_PAGE.width, DEFAULT_PAGE.height), |p| {
                (p.bounds.width, p.bounds.height)
            });

        // Where everything is *now*, and what is standing on it. Both have to
        // be read before a single page moves: afterwards, a frame asked which
        // page it is on would answer with the page's new position, and the
        // whole point is to move it by the difference.
        let before: Vec<(PageId, DocRect, Vec<FrameId>)> = self
            .page_ids()
            .filter_map(|id| {
                let bounds = self.pages.get(id)?.bounds;
                Some((id, bounds, self.frames_on_page(id)))
            })
            .collect();

        let mut y = 0.0;
        for spread in self.spread_order.clone() {
            let pages = self.pages_of(spread);
            // Which side of the fold this spread starts on.
            //
            // A recto is a right-hand page and page one is a recto, so a
            // spread holding page one alone starts in the right-hand column
            // rather than the left. A final lone page is a verso and starts on
            // the left, which is where a column of one would have put it
            // anyway — so only the odd-numbered case needs saying.
            let offset = if self.setup.facing_pages && self.starts_on_a_recto(spread) {
                1
            } else {
                0
            };

            for (column, page) in pages.iter().enumerate() {
                if let Some(page) = self.pages.get_mut(*page) {
                    page.bounds = DocRect {
                        x: (column + offset) as f64 * width,
                        y,
                        width,
                        height,
                    };
                }
            }
            // Past this spread's bleed and slug, and past the next one's, so
            // neither overlaps the other. Both are measured from the trim, so
            // the larger of the two is what stands out.
            y += height + SPREAD_GAP + self.vertical_clearance() * 2.0;
        }

        // A page that moved takes what stands on it. Without this, removing a
        // page slides every page after it upwards and leaves their contents
        // behind — each landing on whichever page has arrived beneath it.
        for (id, was, frames) in before {
            let Some(now) = self.pages.get(id).map(|p| p.bounds) else {
                continue;
            };
            let (dx, dy) = (now.x - was.x, now.y - was.y);
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            for frame in frames {
                self.translate_deeply(frame, dx, dy);
            }
        }

        self.revision += 1;
    }

    /// Whether this spread's first page is odd-numbered in the reading order.
    ///
    /// Odd is a recto: the right-hand page of an opened book. Counted rather
    /// than stored, because a page's number is a consequence of where it falls
    /// in the reading order and storing it would let the two disagree.
    fn starts_on_a_recto(&self, spread: SpreadId) -> bool {
        let Some(first) = self.pages_of(spread).first().copied() else {
            return false;
        };
        self.page_ids()
            .position(|p| p == first)
            .is_some_and(|i| i.is_multiple_of(2))
    }

    /// How far the furthest thing drawn stands above or below a page's trim.
    ///
    /// Bleed and slug are both measured from the trim rather than from each
    /// other, so a slug narrower than the bleed does not read as a negative
    /// one — which means the clearance is the larger of the two, not the sum.
    fn vertical_clearance(&self) -> f64 {
        let bleed = self.setup.bleed.top.max(self.setup.bleed.bottom);
        let slug = self.setup.slug.top.max(self.setup.slug.bottom);
        bleed.max(slug)
    }

    /// Append a page, and say which one it is.
    ///
    /// With facing pages on it joins the last spread when that spread holds
    /// one page and is not the first, and otherwise starts a new one.
    ///
    /// "Not the first" is a rule about cover pages, and there is no getting
    /// away from one: page 1 is a right-hand page, so it stands alone and the
    /// spreads after it pair up — 1, 2-3, 4-5. Joining the last spread without
    /// that exception gives 1-2, 3-4, which reads as though the book opened on
    /// its own cover.
    pub fn add_page(&mut self) -> PageId {
        // Placed by the reflow below; a page is never left where this put it.
        let page = self.pages.insert(Page::at(DEFAULT_PAGE));

        // Appended to the sequence, then the sequence decides the spreads.
        // Doing it the other way round — reasoning about whether the last
        // spread has room — is how the two came to disagree.
        let mut order: Vec<PageId> = self.page_ids().collect();
        order.push(page);
        self.repack_spreads(&order);

        self.reflow_spreads();
        page
    }

    /// Remove a page and everything standing on it.
    ///
    /// Returns whether it removed one. **The last page is refused**: a document
    /// with no pages has nothing to show and no way back except undo, and
    /// InDesign refuses it too.
    pub fn remove_page(&mut self, id: PageId) -> bool {
        if self.page_ids().count() <= 1 || !self.pages.contains_key(id) {
            return false;
        }

        // Which frames stand on it has to be asked *before* the page goes:
        // afterwards every one of them would answer "the nearest page", which
        // is a different page and the wrong one.
        for frame in self.frames_on_page(id) {
            self.remove_frame(frame);
        }
        self.pages.remove(id);

        // The sequence without it, then repacked: taking page two out of
        // 1 | 2-3 | 4-5 has to give 1 | 3-4 | 5, not 1 | 3 | 4-5, or every
        // page after the hole is on the wrong side of the fold.
        let order: Vec<PageId> = self.page_ids().filter(|p| *p != id).collect();
        self.repack_spreads(&order);

        self.reflow_spreads();
        true
    }

    /// Copy a page, everything on it, and the text it says.
    ///
    /// A **deep** copy: a text frame's story is copied too. Sharing it would
    /// make editing one page edit another, which is the same trap
    /// `Command::DuplicateSelection` already avoids for frames — and the one a
    /// reader is least likely to suspect, because the two pages look right
    /// until somebody types.
    ///
    /// The copy is inserted directly after the original's spread.
    pub fn duplicate_page(&mut self, id: PageId) -> Option<PageId> {
        let source = self.pages.get(id)?.clone();
        // Asked before the copy exists, and before the reflow moves anything.
        let standing_on_it = self.frames_on_page(id);

        let mut copy = Page::at(source.bounds);
        // A duplicate keeps its parent: the copy of a page built on a master
        // is a page built on the same master.
        copy.master = source.master;
        let page = self.pages.insert(copy);
        let spread = self.spreads.insert(Spread {
            pages: vec![page],
            guides: self
                .spread_of(id)
                .and_then(|s| self.spreads.get(s))
                .map(|s| s.guides.clone())
                .unwrap_or_default(),
        });

        let at = self
            .spread_of(id)
            .and_then(|s| self.spread_order.iter().position(|o| *o == s))
            .map_or(self.spread_order.len(), |i| i + 1);
        self.spread_order.insert(at, spread);

        // The reflow first, so the new page has its place; only then can the
        // copies be moved onto it.
        self.reflow_spreads();

        // How far the copy sits from the original. Under the old model a
        // frame belonged to its page's layer and so *followed* the page
        // wherever it went; now that a frame's page is where it sits, a copy
        // that is not moved stays on the page it was copied from — two
        // objects stacked on the original, and a blank duplicate.
        let (from, to) = (self.pages.get(id)?.bounds, self.pages.get(page)?.bounds);
        let (dx, dy) = (to.x - from.x, to.y - from.y);

        for frame in standing_on_it {
            let layer = self.layer_of_frame(frame);
            let Some(copy) = self.copy_frame_deeply(frame) else {
                continue;
            };
            self.translate_deeply(copy, dx, dy);
            // Onto the same layer as its original, directly above it. A copy
            // that landed on the active layer instead would jump layers,
            // which is not what duplicating a page means.
            if let Some(layer) = layer.and_then(|l| self.layers.get_mut(l)) {
                layer.frames.push(copy);
            }
        }

        self.revision += 1;
        Some(page)
    }

    /// Move a frame and its children by an offset.
    fn translate_deeply(&mut self, id: FrameId, dx: f64, dy: f64) {
        let children = match self.frames.get_mut(id) {
            Some(frame) => {
                frame.bounds.x += dx;
                frame.bounds.y += dy;
                match &frame.kind {
                    FrameKind::Group(children) => children.clone(),
                    _ => Vec::new(),
                }
            }
            None => return,
        };
        for child in children {
            self.translate_deeply(child, dx, dy);
        }
    }

    /// One frame and its children, with their own stories.
    fn copy_frame_deeply(&mut self, id: FrameId) -> Option<FrameId> {
        let mut frame = self.frames.get(id)?.clone();

        match &mut frame.kind {
            FrameKind::Text { story, .. } => {
                // Its own copy of the words, so the two pages can diverge.
                if let Some(text) = self.stories.get(*story).cloned() {
                    *story = self.stories.insert(text);
                }
            }
            FrameKind::Group(children) => {
                let originals = children.clone();
                children.clear();
                for child in originals {
                    if let Some(copy) = self.copy_frame_deeply(child) {
                        children.push(copy);
                    }
                }
            }
            _ => {}
        }

        Some(self.frames.insert(frame))
    }

    // --- parent pages -------------------------------------------------

    /// Every master, in the order the panel lists them.
    pub fn master_ids(&self) -> impl Iterator<Item = MasterId> + '_ {
        self.master_order.iter().copied()
    }

    /// Add a master spread shaped like the document's own.
    ///
    /// One page when pages do not face, two when they do — a master exists to
    /// be applied to document pages, and one shaped differently could not be.
    pub fn add_master(&mut self, name: impl Into<String>) -> MasterId {
        let bounds = self.first_page_bounds();
        let facing = self.setup.facing_pages;

        let mut pages = vec![self.pages.insert(Page::at(bounds))];
        if facing {
            pages.push(self.pages.insert(Page::at(bounds)));
        }

        // Laid out above the document, where y is negative: the reading order
        // starts at zero and grows downwards, so nothing there can collide
        // with it however many pages are added.
        let gap = bounds.height + SPREAD_GAP;
        let y = -gap * (self.master_order.len() as f64 + 1.0);
        for (column, page) in pages.iter().enumerate() {
            if let Some(page) = self.pages.get_mut(*page) {
                page.bounds = DocRect {
                    x: column as f64 * bounds.width,
                    y,
                    width: bounds.width,
                    height: bounds.height,
                };
            }
        }

        let spread = self.spreads.insert(Spread {
            pages,
            guides: Vec::new(),
        });
        let id = self.masters.insert(Master::new(name, spread));
        self.master_order.push(id);
        self.revision += 1;
        id
    }

    /// A name no existing master has: "A-Master", "B-Master", and so on.
    pub fn unused_master_name(&self) -> String {
        let taken: Vec<&str> = self
            .master_ids()
            .filter_map(|m| self.masters.get(m))
            .map(|m| m.name.as_str())
            .collect();
        for letter in b'A'..=b'Z' {
            let name = format!("{}-Master", letter as char);
            if !taken.contains(&name.as_str()) {
                return name;
            }
        }
        format!("Master {}", self.master_order.len() + 1)
    }

    /// The pages of a master spread.
    pub fn pages_of_master(&self, master: MasterId) -> Vec<PageId> {
        self.masters
            .get(master)
            .map(|m| self.pages_of(m.spread))
            .unwrap_or_default()
    }

    /// Whether this page belongs to a master rather than to the document.
    pub fn is_master_page(&self, page: PageId) -> bool {
        self.master_ids()
            .any(|m| self.pages_of_master(m).contains(&page))
    }

    /// Apply a master to a document page.
    ///
    /// The master page chosen is the one on the **same side of the fold**: a
    /// verso takes the master's verso and a recto takes its recto, which is
    /// what makes a master with different inside and outside margins work at
    /// all. A single-page master applies its one page to either side.
    pub fn apply_master(&mut self, page: PageId, master: Option<MasterId>) -> bool {
        if !self.pages.contains_key(page) || self.is_master_page(page) {
            return false;
        }

        let parent = match master {
            None => None,
            Some(master) => {
                let pages = self.pages_of_master(master);
                if pages.is_empty() {
                    return false;
                }
                // Which column this page sits in, which `reflow_spreads`
                // already decided from its number.
                let column = self
                    .pages
                    .get(page)
                    .map(|p| (p.bounds.x / p.bounds.width.max(1.0)).round() as usize)
                    .unwrap_or(0);
                Some(pages.get(column).copied().unwrap_or(pages[0]))
            }
        };

        let Some(target) = self.pages.get_mut(page) else {
            return false;
        };
        if target.master == parent {
            return false;
        }
        target.master = parent;
        self.revision += 1;
        true
    }

    /// The master items that appear on `page`, and where they land on it.
    ///
    /// Returned as offsets rather than as moved frames, because nothing is
    /// moved: a master item is drawn on every page that inherits it, from one
    /// frame. An item that has been overridden on this page is **left out** —
    /// the local copy stands in its place, and drawing both would double it.
    pub fn inherited_by(&self, page: PageId) -> Vec<(FrameId, f64, f64)> {
        let Some(parent) = self.pages.get(page).and_then(|p| p.master) else {
            return Vec::new();
        };
        let (Some(from), Some(to)) = (self.pages.get(parent), self.pages.get(page)) else {
            return Vec::new();
        };
        let (dx, dy) = (to.bounds.x - from.bounds.x, to.bounds.y - from.bounds.y);

        let replaced: Vec<FrameId> = self
            .frames_on_page(page)
            .iter()
            .filter_map(|f| self.overrides.get(*f).copied())
            .collect();

        self.frames_on_page(parent)
            .into_iter()
            .filter(|f| !replaced.contains(f))
            .map(|f| (f, dx, dy))
            .collect()
    }

    /// Promote a master item to a local copy on `page`.
    ///
    /// The copy is a deep one, stories included, and it lands exactly where
    /// the master item appeared — so overriding an item changes nothing about
    /// how the page looks until the copy is edited, which is the whole point.
    pub fn override_master_item(&mut self, page: PageId, item: FrameId) -> Option<FrameId> {
        let (_, dx, dy) = self
            .inherited_by(page)
            .into_iter()
            .find(|(f, _, _)| *f == item)?;

        let copy = self.copy_frame_deeply(item)?;
        self.translate_deeply(copy, dx, dy);

        let layer = self
            .layer_of_frame(item)
            .filter(|l| self.layers.contains_key(*l))
            .or_else(|| self.default_layer())?;
        if let Some(layer) = self.layers.get_mut(layer) {
            layer.frames.push(copy);
        }
        self.overrides.insert(copy, item);
        self.revision += 1;
        Some(copy)
    }

    /// Take every override on `page` back, so the master shows through again.
    pub fn remove_overrides(&mut self, page: PageId) -> usize {
        let local: Vec<FrameId> = self
            .frames_on_page(page)
            .into_iter()
            .filter(|f| self.overrides.contains_key(*f))
            .collect();

        for frame in &local {
            self.overrides.remove(*frame);
            self.remove_frame(*frame);
        }
        if !local.is_empty() {
            self.revision += 1;
        }
        local.len()
    }

    /// Remove a master, and unhook every page that used it.
    pub fn remove_master(&mut self, id: MasterId) -> bool {
        let Some(master) = self.masters.get(id).cloned() else {
            return false;
        };
        let pages = self.pages_of(master.spread);

        // A page that inherited from it keeps its overrides — they are
        // ordinary frames now, and deleting somebody's work because a master
        // went would be a surprise no undo should have to fix.
        for page in self.page_ids().collect::<Vec<_>>() {
            if self
                .pages
                .get(page)
                .and_then(|p| p.master)
                .is_some_and(|m| pages.contains(&m))
                && let Some(page) = self.pages.get_mut(page)
            {
                page.master = None;
            }
        }

        for page in &pages {
            for frame in self.frames_on_page(*page) {
                self.remove_frame(frame);
            }
            self.pages.remove(*page);
        }
        self.spreads.remove(master.spread);
        self.masters.remove(id);
        self.master_order.retain(|m| *m != id);
        self.revision += 1;
        true
    }

    /// Move a page to another place in the reading order.
    ///
    /// `to` is an index among the pages, not among the spreads: a document is
    /// a sequence of pages, and which spread a page sits on is a consequence
    /// of where it falls in that sequence rather than a fact about the page.
    ///
    /// Spreads are **repacked** afterwards. The first draft of this did not,
    /// on the reasoning that an island spread is something InDesign allows —
    /// and it produced a spread of two pages whose first page was
    /// odd-numbered, which is a contradiction: odd means recto means the
    /// right-hand column, and two pages need both columns. Both were drawn on
    /// the right, the second fell outside the sheet, and the empty left column
    /// could not be dropped onto. A rule derived from the reading order has to
    /// be *kept* consistent with the reading order, not merely computed from
    /// it once.
    pub fn move_page(&mut self, page: PageId, to: usize) -> bool {
        if !self.pages.contains_key(page) {
            return false;
        }

        let mut order: Vec<PageId> = self.page_ids().collect();
        let Some(was) = order.iter().position(|p| *p == page) else {
            return false;
        };
        let to = to.min(order.len().saturating_sub(1));
        if was == to {
            return false;
        }

        order.remove(was);
        order.insert(to, page);
        self.repack_spreads(&order);
        self.reflow_spreads();
        true
    }

    /// Lay a sequence of pages out into spreads.
    ///
    /// Facing pages read 1, 2-3, 4-5: page one is a recto and stands alone,
    /// and the rest pair up. Pages that do not face get one spread each.
    ///
    /// Existing spread objects are **reused in order** rather than rebuilt, so
    /// guides — which belong to a spread rather than to a page — stay on the
    /// spread they were dragged onto instead of following whichever page
    /// happens to land there next.
    fn repack_spreads(&mut self, order: &[PageId]) {
        let mut groups: Vec<Vec<PageId>> = Vec::new();
        let mut rest = order;

        if self.setup.facing_pages && !rest.is_empty() {
            groups.push(vec![rest[0]]);
            rest = &rest[1..];
        }
        let per = if self.setup.facing_pages { 2 } else { 1 };
        for chunk in rest.chunks(per) {
            groups.push(chunk.to_vec());
        }

        let existing = self.spread_order.clone();
        let mut kept = Vec::with_capacity(groups.len());

        for (index, pages) in groups.into_iter().enumerate() {
            let id = match existing.get(index) {
                Some(id) => *id,
                None => self.spreads.insert(Spread {
                    pages: Vec::new(),
                    guides: Vec::new(),
                }),
            };
            if let Some(spread) = self.spreads.get_mut(id) {
                spread.pages = pages;
            }
            kept.push(id);
        }

        // Any spread past the end has no pages left to hold.
        for id in existing.into_iter().skip(kept.len()) {
            self.spreads.remove(id);
        }
        self.spread_order = kept;
    }

    /// Move a spread to another place in the reading order.
    ///
    /// Positions are into `spread_order`, which is what page numbers count.
    pub fn move_spread(&mut self, from: usize, to: usize) {
        if from >= self.spread_order.len() || to >= self.spread_order.len() || from == to {
            return;
        }
        let spread = self.spread_order.remove(from);
        self.spread_order.insert(to, spread);
        self.reflow_spreads();
    }

    pub fn add_frame(&mut self, layer: LayerId, frame: Frame) -> FrameId {
        let id = self.frames.insert(frame);
        if let Some(l) = self.layers.get_mut(layer) {
            l.frames.push(id);
        }
        self.revision += 1;
        id
    }

    /// Remove a frame, and everything inside it if it is a group.
    pub fn remove_frame(&mut self, id: FrameId) {
        for victim in self.descendants(id) {
            self.frames.remove(victim);
            for layer in self.layers.values_mut() {
                layer.frames.retain(|f| *f != victim);
            }
        }
        self.revision += 1;
    }

    pub fn add_story(&mut self, story: Story) -> StoryId {
        self.revision += 1;
        self.stories.insert(story)
    }

    pub fn story(&self, id: StoryId) -> Option<&Story> {
        self.stories.get(id)
    }

    /// Define a named character style and return its id.
    pub fn add_character_style(&mut self, style: CharacterStyle) -> CharacterStyleId {
        self.revision += 1;
        self.character_styles.insert(style)
    }

    /// Define a named paragraph style and return its id.
    pub fn add_paragraph_style(&mut self, style: ParagraphStyle) -> ParagraphStyleId {
        self.revision += 1;
        self.paragraph_styles.insert(style)
    }

    /// Whether basing `child` on `parent` would make a cycle.
    ///
    /// Resolution survives a cycle — it carries a visited set — but a style
    /// based on its own descendant is still nonsense, and the honest place to
    /// refuse it is where it would be created. The style picker asks this
    /// before offering a candidate, so the answer is "not offered" rather than
    /// "rejected".
    pub fn character_based_on_would_cycle(
        &self,
        child: CharacterStyleId,
        parent: CharacterStyleId,
    ) -> bool {
        let mut seen = Vec::new();
        let mut current = Some(parent);
        while let Some(id) = current {
            if id == child {
                return true;
            }
            if seen.contains(&id) {
                // Already broken; adding to it changes nothing.
                return true;
            }
            seen.push(id);
            current = self.character_styles.get(id).and_then(|s| s.based_on);
        }
        false
    }

    /// As above, for paragraph styles.
    pub fn paragraph_based_on_would_cycle(
        &self,
        child: ParagraphStyleId,
        parent: ParagraphStyleId,
    ) -> bool {
        let mut seen = Vec::new();
        let mut current = Some(parent);
        while let Some(id) = current {
            if id == child {
                return true;
            }
            if seen.contains(&id) {
                return true;
            }
            seen.push(id);
            current = self.paragraph_styles.get(id).and_then(|s| s.based_on);
        }
        false
    }

    /// Remove a named character style.
    ///
    /// The caller is responsible for the text that referenced it — see
    /// `Command::DeleteCharacterStyle`, which folds the style into every story
    /// first so that nothing changes appearance. Leaving a dangling reference
    /// is not corruption (`resolve_run` treats an unknown id as saying nothing)
    /// but it does silently drop formatting, which is worse than corruption
    /// because nothing reports it.
    pub fn remove_character_style(&mut self, id: CharacterStyleId) -> Option<CharacterStyle> {
        self.revision += 1;
        self.character_styles.remove(id)
    }

    /// Remove a named paragraph style, under the same caveat.
    pub fn remove_paragraph_style(&mut self, id: ParagraphStyleId) -> Option<ParagraphStyle> {
        self.revision += 1;
        self.paragraph_styles.remove(id)
    }

    /// Bumps the revision, because changing a style changes every run using it.
    ///
    /// This is the one mutation whose effect is entirely indirect: no run and
    /// no paragraph changes, and yet what they draw does. Anything that
    /// memoises on the document has to see it, which is why the bump is here
    /// rather than left to the caller.
    pub fn character_style_mut(&mut self, id: CharacterStyleId) -> Option<&mut CharacterStyle> {
        self.revision += 1;
        self.character_styles.get_mut(id)
    }

    /// As above, for a paragraph style.
    pub fn paragraph_style_mut(&mut self, id: ParagraphStyleId) -> Option<&mut ParagraphStyle> {
        self.revision += 1;
        self.paragraph_styles.get_mut(id)
    }

    /// Bumps the revision on the assumption the caller mutates.
    pub fn story_mut(&mut self, id: StoryId) -> Option<&mut Story> {
        self.revision += 1;
        self.stories.get_mut(id)
    }

    pub fn frame(&self, id: FrameId) -> Option<&Frame> {
        self.frames.get(id)
    }

    /// Bumps the revision on the assumption the caller mutates. Callers that
    /// only want to read must use [`Document::frame`].
    pub fn frame_mut(&mut self, id: FrameId) -> Option<&mut Frame> {
        self.revision += 1;
        self.frames.get_mut(id)
    }

    /// Move a frame within its layer's paint order.
    ///
    /// Order lives in `Layer::frames` rather than in a z-index field on the
    /// frame, so "in front of" is a property of the list and cannot fall out
    /// of sync with itself.
    pub fn move_in_z(&mut self, id: FrameId, how: ZMove) -> bool {
        let Some(layer) = self.layers.values_mut().find(|l| l.frames.contains(&id)) else {
            return false;
        };
        let Some(from) = layer.frames.iter().position(|f| *f == id) else {
            return false;
        };
        let last = layer.frames.len() - 1;
        let to = match how {
            ZMove::Forward => (from + 1).min(last),
            ZMove::Backward => from.saturating_sub(1),
            ZMove::ToFront => last,
            ZMove::ToBack => 0,
        };
        if to == from {
            return false;
        }
        let frame = layer.frames.remove(from);
        layer.frames.insert(to, frame);
        self.revision += 1;
        true
    }

    /// Back-to-front order of the frames a layer owns directly.
    ///
    /// Groups appear as themselves here, not as their children — this is the
    /// order z-moves and hit-testing work in.
    pub fn top_level_order(&self) -> Vec<FrameId> {
        self.layer_ids()
            .filter_map(|l| self.layers.get(l))
            .filter(|l| l.visible)
            .flat_map(|l| l.frames.iter().copied())
            .collect()
    }

    /// Back-to-front paint order, with groups expanded into their children.
    ///
    /// A group has no appearance of its own, so it never appears here; only
    /// the leaves that actually draw do.
    pub fn paint_order(&self) -> Vec<FrameId> {
        let mut out = Vec::new();
        for id in self.top_level_order() {
            self.push_leaves(id, &mut out);
        }
        out
    }

    fn push_leaves(&self, id: FrameId, out: &mut Vec<FrameId>) {
        match self.frames.get(id).map(|f| &f.kind) {
            Some(FrameKind::Group(children)) => {
                for child in children.clone() {
                    self.push_leaves(child, out);
                }
            }
            Some(_) => out.push(id),
            None => {}
        }
    }

    /// Every frame inside `id`, including `id` itself.
    pub fn descendants(&self, id: FrameId) -> Vec<FrameId> {
        let mut out = vec![id];
        if let Some(FrameKind::Group(children)) = self.frames.get(id).map(|f| &f.kind) {
            for child in children.clone() {
                out.extend(self.descendants(child));
            }
        }
        out
    }

    /// The bounds a frame occupies — for a group, the union of its children.
    pub fn effective_bounds(&self, id: FrameId) -> Option<DocRect> {
        let frame = self.frames.get(id)?;
        let FrameKind::Group(children) = &frame.kind else {
            return Some(frame.bounds);
        };
        let mut union: Option<DocRect> = None;
        for child in children {
            let Some(b) = self.effective_bounds(*child) else {
                continue;
            };
            union = Some(match union {
                None => b,
                Some(u) => {
                    let x0 = u.x.min(b.x);
                    let y0 = u.y.min(b.y);
                    let x1 = (u.x + u.width).max(b.x + b.width);
                    let y1 = (u.y + u.height).max(b.y + b.height);
                    DocRect {
                        x: x0,
                        y: y0,
                        width: x1 - x0,
                        height: y1 - y0,
                    }
                }
            });
        }
        union
    }

    /// The axis-aligned box a frame really covers, rotation included.
    ///
    /// [`Document::effective_bounds`] unions children's *unrotated* boxes,
    /// which is right for asking how big a shape is and wrong for drawing a
    /// box around one: a frame turned 45 degrees sticks out well past its own
    /// bounds. Grouping needs the second question answered, or the new group's
    /// box starts out too small.
    pub fn visual_bounds(&self, id: FrameId) -> Option<DocRect> {
        let mut union: Option<(f64, f64, f64, f64)> = None;
        for leaf in self.descendants(id) {
            let Some(frame) = self.frames.get(leaf) else {
                continue;
            };
            if matches!(frame.kind, FrameKind::Group(_)) {
                continue;
            }
            for p in frame.corners() {
                union = Some(match union {
                    None => (p.x, p.y, p.x, p.y),
                    Some((x0, y0, x1, y1)) => (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
                });
            }
        }
        union.map(|(x0, y0, x1, y1)| DocRect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    /// Roughly how much memory this document holds, in bytes.
    ///
    /// Used to bound the undo stack, which holds whole snapshots. Deliberately
    /// an estimate: walking every allocation exactly would cost more than the
    /// bound is worth, and a bound only has to be the right order of
    /// magnitude to stop a large document filling memory with its own history.
    ///
    /// Counts the things that actually scale — frame count, path complexity,
    /// text length — and ignores the fixed overhead of the arenas themselves.
    pub fn footprint(&self) -> usize {
        use std::mem::{size_of, size_of_val};

        let mut bytes = self.frames.len() * size_of::<Frame>();
        for frame in self.frames.values() {
            bytes += match &frame.kind {
                FrameKind::Path(path) => size_of_val(path.elements()),
                FrameKind::Group(children) => children.len() * size_of::<FrameId>(),
                _ => 0,
            };
        }

        for story in self.stories.values() {
            bytes += story.text.len()
                + story.runs.len() * size_of::<tessera_text::story::Run>()
                + size_of::<Story>();
        }

        for layer in self.layers.values() {
            bytes += layer.name.len() + layer.frames.len() * size_of::<FrameId>();
        }
        bytes += self.pages.len() * size_of::<Page>();
        bytes += self.spreads.len() * size_of::<Spread>();
        bytes += self.spread_order.len() * size_of::<SpreadId>();

        bytes
    }

    /// Move a frame, carrying a group's children with it.
    pub fn translate_frame(&mut self, id: FrameId, dx: f64, dy: f64) {
        // Composed onto the placement rather than added to `bounds`. Bounds
        // are in the frame's own space; a translation is in document space,
        // and adding one to the other turns the move by the frame's own angle.
        let by = Transform::translate(dx, dy);
        for leaf in self.descendants(id) {
            if let Some(f) = self.frames.get_mut(leaf) {
                f.transform = f.transform.then(by);
            }
        }
        self.revision += 1;
    }

    /// Collect `ids` into a new group, inserted where the frontmost of them
    /// sat. Returns `None` for fewer than two frames — a group of one is not
    /// a group.
    pub fn group(&mut self, ids: &[FrameId]) -> Option<FrameId> {
        let order = self.top_level_order();
        let mut members: Vec<FrameId> = order
            .iter()
            .copied()
            .filter(|id| ids.contains(id))
            .collect();
        if members.len() < 2 {
            return None;
        }

        let layer_id = self.layer_ids().find(|l| {
            self.layers
                .get(*l)
                .is_some_and(|layer| layer.frames.contains(&members[0]))
        })?;
        let position = self
            .layers
            .get(layer_id)?
            .frames
            .iter()
            .position(|f| Some(f) == members.last())?;

        // The group's own box is authoritative from here on: the interface
        // draws it, and transforms update it alongside the children. So it has
        // to start out enclosing what is actually on screen, rotation and all.
        let bounds = members
            .iter()
            .filter_map(|id| self.visual_bounds(*id))
            .fold(None::<DocRect>, |acc, b| {
                Some(match acc {
                    None => b,
                    Some(u) => {
                        let x0 = u.x.min(b.x);
                        let y0 = u.y.min(b.y);
                        let x1 = (u.x + u.width).max(b.x + b.width);
                        let y1 = (u.y + u.height).max(b.y + b.height);
                        DocRect {
                            x: x0,
                            y: y0,
                            width: x1 - x0,
                            height: y1 - y0,
                        }
                    }
                })
            })?;

        let group = self.frames.insert(Frame {
            bounds,
            kind: FrameKind::Group(std::mem::take(&mut members)),
            transform: Transform::IDENTITY,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        });

        let layer = self.layers.get_mut(layer_id)?;
        // Children leave the layer's list; the group takes their place, at
        // the frontmost member's position so the stack does not jump.
        let FrameKind::Group(children) = &self.frames[group].kind else {
            unreachable!("just inserted a group")
        };
        let children = children.clone();
        // The insertion index must be computed against the list AS IT WILL BE
        // once the children are gone, not as it is now: removing them shifts
        // everything after them down.
        let at = layer
            .frames
            .iter()
            .take(position)
            .filter(|f| !children.contains(f))
            .count();
        layer.frames.retain(|f| !children.contains(f));
        layer.frames.insert(at.min(layer.frames.len()), group);

        self.revision += 1;
        Some(group)
    }

    /// Dissolve a group, returning its children to the layer in its place.
    pub fn ungroup(&mut self, id: FrameId) -> Vec<FrameId> {
        let Some(FrameKind::Group(children)) = self.frames.get(id).map(|f| f.kind.clone()) else {
            return Vec::new();
        };

        let Some(layer_id) = self.layer_ids().find(|l| {
            self.layers
                .get(*l)
                .is_some_and(|layer| layer.frames.contains(&id))
        }) else {
            return Vec::new();
        };

        if let Some(layer) = self.layers.get_mut(layer_id)
            && let Some(at) = layer.frames.iter().position(|f| *f == id)
        {
            layer.frames.remove(at);
            for (offset, child) in children.iter().enumerate() {
                layer.frames.insert(at + offset, *child);
            }
        }

        self.frames.remove(id);
        self.revision += 1;
        children
    }

    /// The topmost frame containing the point, or `None`.
    ///
    /// A rotated frame is tested by rotating the *point* backwards into the
    /// frame's own space, rather than by building a rotated polygon. Same
    /// answer, one line, and it stays correct for any future transform that
    /// is invertible.
    /// A frame's geometry as a path in **document** coordinates, rotation
    /// applied.
    ///
    /// What the frame actually draws, as opposed to the box it draws inside.
    /// A group returns `None`: it has no geometry, only children.
    pub fn outline(&self, id: FrameId) -> Option<kurbo::BezPath> {
        use kurbo::Shape as _;

        let frame = self.frames.get(id)?;
        let b = frame.bounds;
        let rect = kurbo::Rect::new(b.x, b.y, b.x + b.width, b.y + b.height);

        let mut path = match &frame.kind {
            FrameKind::Rectangle | FrameKind::Text { .. } => rect.to_path(ACCURACY),
            FrameKind::Ellipse => kurbo::Ellipse::from_rect(rect).to_path(ACCURACY),
            FrameKind::Path(p) => {
                let mut placed = crate::path::fit_to_bounds(p, b);
                // `fit_to_bounds` answers frame-locally; this is the one place
                // that wants the answer in document space.
                placed.apply_affine(kurbo::Affine::translate((b.x, b.y)));
                placed
            }
            FrameKind::Group(_) => return None,
        };

        path.apply_affine(frame.transform.to_affine());
        Some(path)
    }

    /// The top-level frames a rubber band over `area` catches.
    ///
    /// By content, not by bounding box — the same rule clicking follows. A
    /// marquee that caught anything whose box it grazed would sweep up a
    /// pen-drawn curve from well outside the ink, which is exactly what
    /// clicking was fixed not to do.
    ///
    /// Top-level, also like clicking: a marquee over part of a group takes the
    /// group, because that is what grouping means.
    ///
    /// And **selectable**, like clicking. Reported from real use: locking a
    /// layer stopped a click reaching it but not a rubber band, so a lock could
    /// be walked straight past by dragging a box round it — which is worse than
    /// no lock at all, because it looks like one.
    pub fn frames_touching(&self, area: DocRect) -> Vec<FrameId> {
        self.selectable_order()
            .into_iter()
            .filter(|id| self.touches_anywhere(*id, area))
            .collect()
    }

    fn touches_anywhere(&self, id: FrameId, area: DocRect) -> bool {
        self.descendants(id).into_iter().any(|leaf| {
            let Some(frame) = self.frames.get(leaf) else {
                return false;
            };
            self.outline(leaf)
                .is_some_and(|path| touches(&path, area, is_filled(&frame.kind)))
        })
    }

    /// The frontmost frame `point` lands on, or `None`.
    ///
    /// `tolerance`, in document units, is how far outside a shape's edge still
    /// counts. The viewport passes a few screen pixels converted through the
    /// zoom, so a hairline stays clickable however far out the view is.
    pub fn hit_test(&self, point: DocPoint, tolerance: f64) -> Option<FrameId> {
        // Top level, not paint order: clicking a grouped object selects the
        // GROUP, which is what grouping is for.
        self.selectable_order()
            .into_iter()
            .rev()
            .find(|id| self.hits_anywhere(*id, point, tolerance))
    }

    /// The frames a click or a select-all may reach, back to front.
    ///
    /// [`top_level_order`](Self::top_level_order) minus the locked layers.
    /// The two differ deliberately: a locked layer is **drawn** and not
    /// **touched**, which is the entire use of locking one — you keep a
    /// background visible while you work over it.
    pub fn selectable_order(&self) -> Vec<FrameId> {
        self.layer_ids()
            .filter_map(|l| self.layers.get(l))
            .filter(|l| l.visible && !l.locked)
            .flat_map(|l| l.frames.iter().copied())
            .collect()
    }

    fn hits_anywhere(&self, id: FrameId, point: DocPoint, tolerance: f64) -> bool {
        self.descendants(id).into_iter().any(|leaf| {
            self.frames.get(leaf).is_some_and(|f| {
                !matches!(f.kind, FrameKind::Group(_)) && hits(f, point, tolerance)
            })
        })
    }
}

/// Whether `point` lands on `frame`, accounting for its rotation.
///
/// The shape decides, not the bounding box. A box test is right for a
/// rectangle and wrong for everything else: it hands an ellipse its corners,
/// and it lets a pen-drawn curve claim the whole rectangle it happens to span,
/// so clicking empty space well away from the ink selects it.
fn hits(frame: &Frame, point: DocPoint, tolerance: f64) -> bool {
    let bounds = frame.bounds;
    // Into the frame's own space, where its geometry is described.
    let local = frame.to_local(point);

    match &frame.kind {
        // A text frame is a box, and an empty one still has to be clickable.
        FrameKind::Rectangle | FrameKind::Text { .. } => grown(bounds, tolerance).contains(local),

        FrameKind::Ellipse => {
            let (rx, ry) = (
                bounds.width / 2.0 + tolerance,
                bounds.height / 2.0 + tolerance,
            );
            if rx <= 0.0 || ry <= 0.0 {
                return grown(bounds, tolerance).contains(local);
            }
            let centre = bounds.center();
            let nx = (local.x - centre.x) / rx;
            let ny = (local.y - centre.y) / ry;
            nx * nx + ny * ny <= 1.0
        }

        // Proximity to the ink, not to the box. A path frame renders as a
        // stroke and never as a fill (see `tessera_layout::resolve`), so the
        // empty middle of a closed pen shape belongs to whatever is behind it
        // — which is what an unfilled path means in every layout tool.
        FrameKind::Path(path) => {
            use kurbo::ParamCurveNearest as _;

            let reach = tolerance + frame.stroke.as_ref().map_or(1.0, |s| s.width) / 2.0;
            // `fit_to_bounds` answers in frame-local coordinates, so the
            // point has to be asked the same way.
            let fitted = crate::path::fit_to_bounds(path, bounds);
            let at = kurbo::Point::new(local.x - bounds.x, local.y - bounds.y);
            // Cheap rejection first: a path cannot be nearer than its own box.
            if !grown(bounds, reach).contains(local) {
                return false;
            }
            fitted
                .segments()
                .any(|seg| seg.nearest(at, ACCURACY).distance_sq <= reach * reach)
        }

        // A group is a container. `hits_anywhere` asks its children instead.
        FrameKind::Group(_) => false,
    }
}

/// Whether a marquee lying wholly inside this shape should catch it.
///
/// A filled shape swallows the band; an unfilled outline does not, for the
/// same reason clicking its empty middle does not select it.
fn is_filled(kind: &FrameKind) -> bool {
    matches!(
        kind,
        FrameKind::Rectangle | FrameKind::Ellipse | FrameKind::Text { .. }
    )
}

/// Whether the rubber band over `area` touches `path`.
///
/// Three ways it can: the band contains part of the path, the path crosses an
/// edge of the band, or — for a filled shape — the band is entirely inside it.
fn touches(path: &kurbo::BezPath, area: DocRect, filled: bool) -> bool {
    use kurbo::{ParamCurve as _, Shape as _};

    let band = kurbo::Rect::new(area.x, area.y, area.x + area.width, area.y + area.height);
    let corners = [
        (band.x0, band.y0),
        (band.x1, band.y0),
        (band.x1, band.y1),
        (band.x0, band.y1),
    ];
    let edges: Vec<kurbo::Line> = (0..4)
        .map(|i| kurbo::Line::new(corners[i], corners[(i + 1) % 4]))
        .collect();

    for seg in path.segments() {
        if band.contains(seg.eval(0.0)) || band.contains(seg.eval(1.0)) {
            return true;
        }
        // Exact against the curve, not against a flattened approximation of
        // it: a band clipping the bulge of a tight arc must still catch it.
        if edges
            .iter()
            .any(|edge| !seg.intersect_line(*edge).is_empty())
        {
            return true;
        }
    }

    filled && path.winding(band.center()) != 0
}

/// How precisely a curve's nearest point is found, in document units. Well
/// below anything a pointer can express, and far cheaper than exact.
const ACCURACY: f64 = 0.05;

/// `bounds` grown by `by` on every side.
fn grown(bounds: DocRect, by: f64) -> DocRect {
    DocRect {
        x: bounds.x - by,
        y: bounds.y - by,
        width: bounds.width + by * 2.0,
        height: bounds.height + by * 2.0,
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

/// The document is where the named styles live, so it is what resolves them.
///
/// The trait is declared in `tessera_text`, which has no idea what a document
/// is — that isolation is what keeps shaping and caret movement testable
/// headless. This is the one place the two meet.
impl Styles for Document {
    fn character(&self, id: CharacterStyleId) -> Option<&CharacterFormat> {
        self.character_styles.get(id).map(|s| &s.format)
    }

    fn paragraph(&self, id: ParagraphStyleId) -> Option<&ParagraphFormat> {
        self.paragraph_styles.get(id).map(|s| &s.format)
    }

    fn character_parent(&self, id: CharacterStyleId) -> Option<CharacterStyleId> {
        self.character_styles.get(id).and_then(|s| s.based_on)
    }

    fn paragraph_parent(&self, id: ParagraphStyleId) -> Option<ParagraphStyleId> {
        self.paragraph_styles.get(id).and_then(|s| s.based_on)
    }

    fn document_default(&self) -> CharacterFormat {
        let d = &self.text_default;
        CharacterFormat {
            family: Some(d.family.clone()),
            size: Some(d.size),
            line_height: Some(d.line_height),
            colour: Some(d.color.clone()),
            ..CharacterFormat::default()
        }
    }
}

/// `rect` pulled inward by a different amount on each edge, never past
/// inside-out.
///
/// An inverted rectangle draws as a shape turned inside out and hit-tests as
/// nothing, so a margin wider than the page collapses to zero instead.
fn inset_each(rect: DocRect, top: f64, bottom: f64, left: f64, right: f64) -> DocRect {
    DocRect {
        x: rect.x + left,
        y: rect.y + top,
        width: (rect.width - left - right).max(0.0),
        height: (rect.height - top - bottom).max(0.0),
    }
}

/// `rect` pushed outward by a different amount on each edge.
///
/// Named apart from the two-argument `grown` above, which insets uniformly.
fn outset_each(rect: DocRect, top: f64, bottom: f64, left: f64, right: f64) -> DocRect {
    DocRect {
        x: rect.x - left,
        y: rect.y - top,
        width: rect.width + left + right,
        height: rect.height + top + bottom,
    }
}

#[cfg(test)]
mod tests {
    use crate::nodes::{Axis, Guide, Insets, Margins, PageSide};

    fn guide(position: f64) -> Guide {
        Guide {
            axis: Axis::Vertical,
            position,
            locked: false,
        }
    }

    #[test]
    fn changing_the_setup_moves_the_revision() {
        // The resolve cache is keyed on this. Without the bump the canvas
        // keeps drawing the old margins until something else changes.
        let mut doc = super::Document::new();
        let before = doc.revision();
        let mut setup = doc.setup;
        setup.margins = Margins::uniform(36.0);
        doc.set_setup(setup);
        assert_ne!(doc.revision(), before);
    }

    #[test]
    fn resizing_the_pages_moves_the_revision() {
        let mut doc = super::Document::new();
        let before = doc.revision();
        doc.set_page_size(595.0, 842.0);
        assert_ne!(doc.revision(), before);
        for page in doc.pages.values() {
            assert_eq!((page.bounds.width, page.bounds.height), (595.0, 842.0));
        }
    }

    #[test]
    fn a_guide_added_to_a_spread_can_be_read_back() {
        let mut doc = super::Document::new();
        let spread = doc.spread_ids().next().expect("a spread");
        doc.add_guide(spread, guide(120.0));

        let guides = doc.guides_of(spread);
        assert_eq!(guides.len(), 1);
        assert_eq!(guides[0].position, 120.0);
        assert_eq!(guides[0].axis, Axis::Vertical);
    }

    #[test]
    fn adding_a_guide_moves_the_revision_so_the_canvas_redraws() {
        let mut doc = super::Document::new();
        let spread = doc.spread_ids().next().expect("a spread");
        let before = doc.revision();
        doc.add_guide(spread, guide(40.0));
        assert_ne!(doc.revision(), before);
    }

    #[test]
    fn removing_a_guide_returns_it_and_leaves_the_rest() {
        let mut doc = super::Document::new();
        let spread = doc.spread_ids().next().expect("a spread");
        for position in [10.0, 20.0, 30.0] {
            doc.add_guide(spread, guide(position));
        }
        let removed = doc.remove_guide(spread, 1).expect("the middle one");
        assert_eq!(removed.position, 20.0);
        assert_eq!(
            doc.guides_of(spread)
                .iter()
                .map(|g| g.position)
                .collect::<Vec<_>>(),
            vec![10.0, 30.0]
        );
    }

    #[test]
    fn removing_a_guide_that_is_not_there_returns_nothing() {
        let mut doc = super::Document::new();
        let spread = doc.spread_ids().next().expect("a spread");
        assert!(doc.remove_guide(spread, 7).is_none());
    }

    #[test]
    fn a_page_has_no_side_when_pages_do_not_face() {
        let doc = super::Document::new();
        let page = doc.page_ids().next().expect("a new document has a page");
        assert_eq!(doc.page_side(page), PageSide::Single);
    }

    #[test]
    fn the_first_page_of_a_facing_spread_is_a_verso_and_the_second_a_recto() {
        let mut doc = super::Document::new();
        doc.setup.facing_pages = true;

        let spread = doc.spread_ids().next().expect("a spread");
        let first = doc.pages_of(spread)[0];
        // A one-page spread still has no facing partner.
        assert_eq!(doc.page_side(first), PageSide::Single);

        let second = doc.add_page_to(spread);
        assert_eq!(doc.page_side(first), PageSide::Left, "verso");
        assert_eq!(doc.page_side(second), PageSide::Right, "recto");
    }

    #[test]
    fn a_page_knows_which_spread_holds_it() {
        let doc = super::Document::new();
        let page = doc.page_ids().next().expect("a page");
        let spread = doc.spread_of(page).expect("it is in a spread");
        assert!(doc.pages_of(spread).contains(&page));
    }

    #[test]
    fn margins_inset_the_page_from_every_edge() {
        let mut doc = super::Document::new();
        doc.setup.margins = Margins::uniform(36.0);
        let page = doc.page_ids().next().expect("a page");
        let bounds = doc.pages[page].bounds;

        let inner = doc.margin_rect(page).expect("a margin rect");
        assert_eq!(inner.x, bounds.x + 36.0);
        assert_eq!(inner.y, bounds.y + 36.0);
        assert_eq!(inner.width, bounds.width - 72.0);
        assert_eq!(inner.height, bounds.height - 72.0);
    }

    #[test]
    fn the_inside_margin_swaps_sides_between_verso_and_recto() {
        // The whole reason margins are inside/outside rather than left/right.
        let mut doc = super::Document::new();
        doc.setup.facing_pages = true;
        doc.setup.margins = Margins {
            top: 10.0,
            bottom: 10.0,
            inside: 60.0,
            outside: 20.0,
        };
        let spread = doc.spread_ids().next().expect("a spread");
        let verso = doc.pages_of(spread)[0];
        let recto = doc.add_page_to(spread);

        let v = doc.margin_rect(verso).expect("verso");
        let r = doc.margin_rect(recto).expect("recto");
        let vb = doc.pages[verso].bounds;
        let rb = doc.pages[recto].bounds;

        assert_eq!(v.x, vb.x + 20.0, "verso: the outside margin is on the left");
        assert_eq!(r.x, rb.x + 60.0, "recto: the inside margin is on the left");
    }

    #[test]
    fn bleed_grows_outward_rather_than_inward() {
        let mut doc = super::Document::new();
        doc.setup.bleed = Insets::uniform(9.0);
        let page = doc.page_ids().next().expect("a page");
        let bounds = doc.pages[page].bounds;

        let bleed = doc.bleed_rect(page).expect("a bleed rect");
        assert_eq!(bleed.x, bounds.x - 9.0);
        assert_eq!(bleed.width, bounds.width + 18.0);
    }

    #[test]
    fn the_slug_lies_outside_the_bleed() {
        let mut doc = super::Document::new();
        doc.setup.bleed = Insets::uniform(9.0);
        doc.setup.slug = Insets::uniform(18.0);
        let page = doc.page_ids().next().expect("a page");

        let bleed = doc.bleed_rect(page).expect("bleed");
        let slug = doc.slug_rect(page).expect("slug");
        assert!(slug.x < bleed.x, "the slug is further out than the bleed");
        assert!(slug.width > bleed.width);
    }

    #[test]
    fn margins_wider_than_the_page_collapse_rather_than_inverting() {
        // An inside-out rectangle draws as a shape turned inside out and
        // hit-tests as nothing. Collapsing to zero is the honest degenerate.
        let mut doc = super::Document::new();
        doc.setup.margins = Margins::uniform(10_000.0);
        let page = doc.page_ids().next().expect("a page");
        let inner = doc.margin_rect(page).expect("a rect");
        assert!(inner.width >= 0.0 && inner.height >= 0.0);
    }

    use super::*;
    use crate::nodes::{Frame, FrameKind};
    use tessera_color::Color;
    use tessera_geometry::{DocPoint, DocRect};

    /// A frame standing on `page`.
    ///
    /// Under the old model this was `add_frame(page.layers[0], ..)` and the
    /// geometry did not matter. Now the geometry is the *only* thing that
    /// decides, which is the point of the change.
    fn frame_on(doc: &mut Document, page: PageId) -> FrameId {
        let on = doc.pages[page].bounds;
        let layer = doc.default_layer().expect("a layer");
        let mut frame = rect_frame();
        frame.bounds = DocRect {
            x: on.x + 10.0,
            y: on.y + 10.0,
            width: 40.0,
            height: 30.0,
        };
        doc.add_frame(layer, frame)
    }

    fn rect_frame() -> Frame {
        Frame {
            bounds: DocRect {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn a_new_document_has_one_spread_with_one_page_and_one_layer() {
        let doc = Document::new();
        assert_eq!(doc.spread_ids().count(), 1);
        assert_eq!(doc.page_ids().count(), 1);
        assert_eq!(doc.layer_ids().count(), 1);
    }

    #[test]
    fn a_new_document_is_us_letter_in_points() {
        let doc = Document::new();
        let page = doc.first_page_bounds();
        assert_eq!((page.width, page.height), (612.0, 792.0));
    }

    #[test]
    fn an_added_frame_can_be_read_back() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let id = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.frame(id).expect("frame exists").bounds.width, 100.0);
    }

    #[test]
    fn adding_a_frame_advances_the_revision() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let before = doc.revision();
        doc.add_frame(layer, rect_frame());
        assert!(doc.revision() > before);
    }

    #[test]
    fn a_removed_frame_is_gone_from_the_arena_and_from_its_layer() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let id = doc.add_frame(layer, rect_frame());
        doc.remove_frame(id);
        assert!(doc.frame(id).is_none());
        assert!(doc.paint_order().is_empty());
    }

    #[test]
    fn hit_test_finds_a_frame_under_the_point_and_nothing_outside_it() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let id = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }, 0.0), Some(id));
        assert_eq!(doc.hit_test(DocPoint { x: 5.0, y: 5.0 }, 0.0), None);
    }

    #[test]
    fn hit_test_returns_the_topmost_frame() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        let _under = doc.add_frame(layer, rect_frame());
        let over = doc.add_frame(layer, rect_frame());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }, 0.0), Some(over));
    }

    #[test]
    fn a_hidden_layer_contributes_nothing_to_paint_order() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        doc.add_frame(layer, rect_frame());
        doc.layers.get_mut(layer).expect("layer").visible = false;
        assert!(doc.paint_order().is_empty());
    }

    #[test]
    fn a_hidden_layer_cannot_be_hit() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("default layer");
        doc.add_frame(layer, rect_frame());
        doc.layers.get_mut(layer).expect("layer").visible = false;
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 40.0 }, 0.0), None);
    }

    #[test]
    fn bringing_a_frame_forward_swaps_it_with_the_one_above() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, rect_frame());
        let b = doc.add_frame(layer, rect_frame());

        assert!(doc.move_in_z(a, ZMove::Forward));

        assert_eq!(doc.paint_order(), vec![b, a]);
    }

    #[test]
    fn sending_to_back_puts_a_frame_first_in_paint_order() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, rect_frame());
        let b = doc.add_frame(layer, rect_frame());
        let c = doc.add_frame(layer, rect_frame());

        assert!(doc.move_in_z(c, ZMove::ToBack));

        assert_eq!(doc.paint_order(), vec![c, a, b]);
    }

    #[test]
    fn moving_the_frontmost_frame_forward_changes_nothing() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, rect_frame());
        let b = doc.add_frame(layer, rect_frame());

        assert!(!doc.move_in_z(b, ZMove::Forward), "already at the front");

        assert_eq!(doc.paint_order(), vec![a, b]);
    }

    #[test]
    fn z_order_decides_which_frame_a_click_finds() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let under = doc.add_frame(layer, rect_frame());
        let over = doc.add_frame(layer, rect_frame());
        let point = DocPoint { x: 50.0, y: 40.0 };

        assert_eq!(doc.hit_test(point, 0.0), Some(over));
        doc.move_in_z(over, ZMove::ToBack);
        assert_eq!(
            doc.hit_test(point, 0.0),
            Some(under),
            "the stack really reordered"
        );
    }

    #[test]
    fn moving_an_unknown_frame_reports_failure_rather_than_panicking() {
        let mut doc = Document::new();
        assert!(!doc.move_in_z(FrameId::default(), ZMove::ToFront));
    }

    /// A long thin bar, so rotating it moves real area around.
    fn bar() -> Frame {
        Frame {
            bounds: DocRect {
                x: 0.0,
                y: 45.0,
                width: 100.0,
                height: 10.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    #[test]
    fn an_unrotated_frame_hit_tests_as_before() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, bar());
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0), Some(id));
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 10.0 }, 0.0), None);
    }

    #[test]
    fn rotating_a_frame_moves_where_it_can_be_hit() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, bar());

        // Upright, the bar spans x 0..100 at y 45..55, centred on (50, 50).
        // Turned a quarter turn it spans y 0..100 at x 45..55.
        let frame = doc.frames.get_mut(id).expect("frame");
        frame.transform = Transform::rotate_about(90.0, frame.bounds.center());

        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 10.0 }, 0.0),
            Some(id),
            "the bar now reaches up the page"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 10.0, y: 50.0 }, 0.0),
            None,
            "and no longer reaches across it"
        );
    }

    #[test]
    fn the_centre_of_a_rotated_frame_is_always_a_hit() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, bar());
        for angle in [0.0, 17.0, 45.0, 90.0, 180.0, -33.0] {
            let frame = doc.frames.get_mut(id).expect("frame");
            frame.transform = Transform::rotate_about(angle, frame.bounds.center());
            assert_eq!(
                doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0),
                Some(id),
                "rotation {angle} lost its own centre"
            );
        }
    }

    /// A rectangle at a given position, 20x20.
    fn at(x: f64, y: f64) -> Frame {
        Frame {
            bounds: DocRect {
                x,
                y,
                width: 20.0,
                height: 20.0,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    /// Two rectangles, side by side, in one layer.
    fn two_apart() -> (Document, FrameId, FrameId) {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, at(0.0, 0.0));
        let b = doc.add_frame(layer, at(100.0, 0.0));
        (doc, a, b)
    }

    // --- shape-precise hit testing -------------------------------------

    fn shape(kind: FrameKind, bounds: DocRect) -> Frame {
        Frame {
            bounds,
            kind,
            transform: Transform::IDENTITY,
            fill: tessera_color::Color::BLACK,
            stroke: None,
        }
    }

    fn square() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }
    }

    fn with(kind: FrameKind) -> (Document, FrameId) {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, shape(kind, square()));
        (doc, id)
    }

    #[test]
    fn an_ellipse_does_not_claim_its_corners() {
        // The bug this pins: every kind was hit-tested against its bounding
        // box, so the empty corner of a circle selected it.
        let (doc, id) = with(FrameKind::Ellipse);
        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0),
            Some(id),
            "the middle is still inside"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 3.0, y: 3.0 }, 0.0),
            None,
            "the corner of the box is outside the ellipse"
        );
    }

    /// A diagonal stroke from corner to corner of `square`.
    fn diagonal() -> FrameKind {
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 100.0));
        FrameKind::Path(path)
    }

    #[test]
    fn a_path_is_hit_on_its_ink_and_not_across_its_box() {
        // The reported bug: a curve drawn with the pen could be selected by
        // clicking anywhere inside the rectangle it happened to span.
        let (doc, id) = with(diagonal());
        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0),
            Some(id),
            "on the line"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 90.0, y: 10.0 }, 0.0),
            None,
            "well off the line but inside its box"
        );
    }

    #[test]
    fn a_closed_path_does_not_claim_its_empty_middle() {
        // A path frame renders as a stroke and never as a fill, so its inside
        // belongs to whatever is behind it.
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 0.0));
        path.line_to((100.0, 100.0));
        path.line_to((0.0, 100.0));
        path.close_path();
        let (doc, id) = with(FrameKind::Path(path));

        assert_eq!(doc.hit_test(DocPoint { x: 0.5, y: 50.0 }, 0.0), Some(id));
        assert_eq!(
            doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0),
            None,
            "the middle of an unfilled outline is not the outline"
        );
    }

    #[test]
    fn tolerance_makes_a_hairline_clickable_without_making_it_a_box() {
        let (doc, id) = with(diagonal());
        let just_off = DocPoint { x: 53.0, y: 50.0 };
        assert_eq!(doc.hit_test(just_off, 0.0), None, "no tolerance, no hit");
        assert_eq!(
            doc.hit_test(just_off, 4.0),
            Some(id),
            "a few units of slack catches it"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 90.0, y: 10.0 }, 4.0),
            None,
            "but slack must not restore the bounding box"
        );
    }

    #[test]
    fn a_rotated_path_is_hit_where_it_was_drawn_to() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut frame = shape(diagonal(), square());
        frame.transform = Transform::rotate_about(90.0, frame.bounds.center());
        let id = doc.add_frame(layer, frame);

        // The centre is on the line whatever the angle...
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0), Some(id));
        // ...and the far corner, which the rotated line now passes through.
        assert_eq!(doc.hit_test(DocPoint { x: 10.0, y: 90.0 }, 2.0), Some(id));
        assert_eq!(
            doc.hit_test(DocPoint { x: 90.0, y: 90.0 }, 2.0),
            None,
            "the corner the rotated line moved away from"
        );
    }

    #[test]
    fn a_text_frame_is_still_a_box_even_when_it_is_empty() {
        // An empty text frame has no ink at all. Hit-testing it by its
        // content would make it impossible to select or delete.
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(tessera_text::story::Story::default());
        let id = doc.add_frame(layer, shape(FrameKind::text(story), square()));
        assert_eq!(doc.hit_test(DocPoint { x: 50.0, y: 50.0 }, 0.0), Some(id));
    }

    // --- the marquee catches content, not boxes --------------------------

    fn band(x: f64, y: f64, width: f64, height: f64) -> DocRect {
        DocRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_marquee_across_a_curve_catches_it() {
        let (doc, id) = with(diagonal());
        // A band straddling the middle of the diagonal.
        assert_eq!(
            doc.frames_touching(band(40.0, 40.0, 20.0, 20.0)),
            vec![id],
            "the band crosses the ink"
        );
    }

    #[test]
    fn a_marquee_in_the_empty_part_of_a_curves_box_catches_nothing() {
        // The reported bug: selecting by bounding box swept up a pen-drawn
        // curve from a corner of its box the ink never reaches.
        let (doc, _) = with(diagonal());
        assert!(
            doc.frames_touching(band(80.0, 5.0, 15.0, 15.0)).is_empty(),
            "the band is inside the box but nowhere near the curve"
        );
    }

    #[test]
    fn a_marquee_wholly_inside_a_filled_shape_still_catches_it() {
        // Otherwise a rubber band drawn in the middle of a large rectangle
        // would select nothing, which is not what any layout tool does.
        let (doc, id) = with(FrameKind::Rectangle);
        assert_eq!(doc.frames_touching(band(40.0, 40.0, 5.0, 5.0)), vec![id]);
    }

    #[test]
    fn a_marquee_inside_an_unfilled_outline_catches_nothing() {
        // The same rule clicking follows: an unfilled path is its outline.
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 0.0));
        path.line_to((100.0, 100.0));
        path.line_to((0.0, 100.0));
        path.close_path();
        let (doc, _) = with(FrameKind::Path(path));
        assert!(doc.frames_touching(band(40.0, 40.0, 5.0, 5.0)).is_empty());
    }

    #[test]
    fn a_marquee_that_misses_entirely_catches_nothing() {
        let (doc, _) = with(FrameKind::Rectangle);
        assert!(
            doc.frames_touching(band(500.0, 500.0, 10.0, 10.0))
                .is_empty()
        );
    }

    #[test]
    fn a_marquee_over_part_of_a_group_takes_the_whole_group() {
        // Top-level, exactly as clicking is. Selecting one child out of a
        // group by rubber band would contradict what grouping means — and
        // paint order, which this used to walk, only ever yields children.
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");
        assert_eq!(
            doc.frames_touching(band(-5.0, -5.0, 20.0, 20.0)),
            vec![g],
            "the band touches only the first child"
        );
    }

    #[test]
    fn a_marquee_catches_a_rotated_frame_where_it_really_is() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut frame = shape(FrameKind::Rectangle, square());
        frame.transform = Transform::rotate_about(45.0, frame.bounds.center());
        let id = doc.add_frame(layer, frame);

        // A 45-degree square reaches further along the axes through its
        // centre than its unrotated bounds do.
        let reach = 50.0 * 2.0_f64.sqrt();
        let outside = 50.0 + reach - 4.0;
        assert_eq!(
            doc.frames_touching(band(outside, 48.0, 3.0, 3.0)),
            vec![id],
            "the corner swung out to here"
        );
    }

    #[test]
    fn grouping_needs_at_least_two_frames() {
        let (mut doc, a, _) = two_apart();
        assert!(doc.group(&[a]).is_none(), "a group of one is not a group");
        assert!(doc.group(&[]).is_none());
    }

    #[test]
    fn a_group_takes_the_union_of_its_children() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");
        let bounds = doc.effective_bounds(g).expect("bounds");
        assert_eq!(bounds.x, 0.0);
        assert_eq!(bounds.width, 120.0, "0..20 and 100..120");
    }

    #[test]
    fn grouping_does_not_change_what_is_painted() {
        let (mut doc, a, b) = two_apart();
        let before = doc.paint_order();
        doc.group(&[a, b]).expect("grouped");
        assert_eq!(
            doc.paint_order(),
            before,
            "a group has no appearance of its own, so the leaves are unchanged"
        );
    }

    #[test]
    fn the_group_replaces_its_children_at_the_top_level() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");
        let top = doc.top_level_order();
        assert_eq!(top, vec![g], "children leave the layer's own list");
    }

    #[test]
    fn clicking_a_grouped_object_selects_the_group() {
        let (mut doc, a, b) = two_apart();
        assert_eq!(doc.hit_test(DocPoint { x: 10.0, y: 10.0 }, 0.0), Some(a));

        let g = doc.group(&[a, b]).expect("grouped");

        assert_eq!(
            doc.hit_test(DocPoint { x: 10.0, y: 10.0 }, 0.0),
            Some(g),
            "the group answers for its children"
        );
        assert_eq!(
            doc.hit_test(DocPoint { x: 110.0, y: 10.0 }, 0.0),
            Some(g),
            "either child, same answer"
        );
    }

    #[test]
    fn the_gap_between_grouped_objects_is_not_a_hit() {
        // A group is its children, not their bounding box. Clicking the empty
        // space between two grouped objects must miss.
        let (mut doc, a, b) = two_apart();
        doc.group(&[a, b]).expect("grouped");
        assert_eq!(doc.hit_test(DocPoint { x: 60.0, y: 10.0 }, 0.0), None);
    }

    #[test]
    fn moving_a_group_carries_its_children() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");

        doc.translate_frame(g, 5.0, 7.0);

        // Where the children really are. A move is a change of placement, so
        // their own boxes are untouched.
        assert_eq!(doc.frame(a).expect("a").corners()[0].x, 5.0);
        assert_eq!(doc.frame(b).expect("b").corners()[0].x, 105.0);
        assert_eq!(doc.frame(a).expect("a").corners()[0].y, 7.0);
        assert_eq!(
            doc.frame(a).expect("a").bounds.x,
            0.0,
            "the frame's own space did not move"
        );
    }

    #[test]
    fn translating_a_rotated_frame_moves_it_the_way_it_was_asked_to() {
        // The reported bug: a translation was added straight into `bounds`,
        // which is in the frame's own space, so it came out turned by the
        // frame's own angle -- and at a half turn, exactly backwards.
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let id = doc.add_frame(layer, at(0.0, 0.0));

        for degrees in [0.0, 90.0, 180.0, -37.0] {
            let frame = doc.frames.get_mut(id).expect("frame");
            let centre = frame.bounds.center();
            frame.transform = Transform::rotate_about(degrees, centre);
            let before = doc.frame(id).expect("frame").centre();

            doc.translate_frame(id, 20.0, 0.0);

            let after = doc.frame(id).expect("frame").centre();
            assert!(
                (after.x - before.x - 20.0).abs() < 1e-9 && (after.y - before.y).abs() < 1e-9,
                "at {degrees} degrees it moved {:?} rather than 20 to the right",
                (after.x - before.x, after.y - before.y)
            );
        }
    }

    #[test]
    fn ungrouping_returns_the_children_in_place() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");

        let freed = doc.ungroup(g);

        assert_eq!(freed, vec![a, b]);
        assert_eq!(doc.top_level_order(), vec![a, b]);
        assert!(doc.frame(g).is_none(), "the group itself is gone");
    }

    #[test]
    fn grouping_then_ungrouping_restores_the_stack() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, at(0.0, 0.0));
        let b = doc.add_frame(layer, at(100.0, 0.0));
        let c = doc.add_frame(layer, at(200.0, 0.0));
        let before = doc.top_level_order();

        let g = doc.group(&[a, b]).expect("grouped");
        doc.ungroup(g);

        assert_eq!(doc.top_level_order(), before);
        assert_eq!(before, vec![a, b, c]);
    }

    #[test]
    fn deleting_a_group_deletes_its_children_too() {
        let (mut doc, a, b) = two_apart();
        let g = doc.group(&[a, b]).expect("grouped");

        doc.remove_frame(g);

        assert!(
            doc.frame(a).is_none(),
            "an orphaned child would be invisible"
        );
        assert!(doc.frame(b).is_none());
        assert!(doc.paint_order().is_empty());
    }

    #[test]
    fn ungrouping_something_that_is_not_a_group_does_nothing() {
        let (mut doc, a, _) = two_apart();
        assert!(doc.ungroup(a).is_empty());
        assert!(doc.frame(a).is_some(), "and does not destroy it");
    }

    #[test]
    fn a_group_can_be_moved_in_z_as_one_object() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, at(0.0, 0.0));
        let b = doc.add_frame(layer, at(100.0, 0.0));
        let c = doc.add_frame(layer, at(200.0, 0.0));

        let g = doc.group(&[a, b]).expect("grouped");
        assert_eq!(doc.top_level_order(), vec![g, c]);

        doc.move_in_z(g, ZMove::ToFront);

        assert_eq!(doc.top_level_order(), vec![c, g]);
        assert_eq!(
            doc.paint_order(),
            vec![c, a, b],
            "the children move with it, keeping their order"
        );
    }

    #[test]
    fn nested_groups_report_every_descendant() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let a = doc.add_frame(layer, at(0.0, 0.0));
        let b = doc.add_frame(layer, at(100.0, 0.0));
        let c = doc.add_frame(layer, at(200.0, 0.0));

        let inner = doc.group(&[a, b]).expect("inner");
        let outer = doc.group(&[inner, c]).expect("outer");

        let mut found = doc.descendants(outer);
        found.sort_by_key(|k| format!("{k:?}"));
        let mut expected = vec![outer, inner, a, b, c];
        expected.sort_by_key(|k| format!("{k:?}"));
        assert_eq!(found, expected);
        assert_eq!(doc.paint_order(), vec![a, b, c], "leaves only, in order");
    }

    // --- more than one page -------------------------------------------------

    #[test]
    fn a_new_document_has_one_page_and_adding_gives_it_a_second() {
        let mut doc = Document::new();
        assert_eq!(doc.page_ids().count(), 1);
        doc.add_page();
        assert_eq!(doc.page_ids().count(), 2);
    }

    #[test]
    fn a_second_page_does_not_sit_on_top_of_the_first() {
        // Bounds are document space and everything downstream reads them: the
        // rulers, align-to-page, the PDF's TrimBox. Two pages in one place
        // would be wrong everywhere at once.
        let mut doc = Document::new();
        let first = doc.page_ids().next().expect("a page");
        let second = doc.add_page();

        let a = doc.pages[first].bounds;
        let b = doc.pages[second].bounds;
        assert!(
            b.y >= a.y + a.height || b.x >= a.x + a.width,
            "the second page overlaps the first: {a:?} then {b:?}"
        );
    }

    #[test]
    fn facing_pages_reads_one_then_two_and_three() {
        // InDesign's pattern, and the test that corrected the decision behind
        // it. Page 1 is a right-hand page, so it stands alone; pages 2 and 3
        // face each other. Joining the last spread unconditionally would give
        // 1-2, 3-4, which reads as though the book opened on its own cover.
        let mut doc = Document::new();
        doc.setup.facing_pages = true;
        let first = doc.page_ids().next().expect("a page");
        let second = doc.add_page();
        let third = doc.add_page();

        assert_eq!(doc.spread_order.len(), 2, "two spreads for three pages");
        assert_eq!(
            doc.pages_of(doc.spread_order[0]),
            vec![first],
            "page one stands alone"
        );
        assert_eq!(
            doc.pages_of(doc.spread_order[1]),
            vec![second, third],
            "and pages two and three face each other"
        );
    }

    #[test]
    fn two_facing_pages_sit_beside_each_other() {
        let mut doc = Document::new();
        doc.setup.facing_pages = true;
        let second = doc.add_page();
        let third = doc.add_page();

        let a = doc.pages[second].bounds;
        let b = doc.pages[third].bounds;
        assert!((a.y - b.y).abs() < 1e-9, "level with each other");
        assert!(
            (b.x - (a.x + a.width)).abs() < 1e-9,
            "and touching: {a:?} then {b:?}"
        );
    }

    #[test]
    fn a_fourth_page_starts_the_next_spread() {
        let mut doc = Document::new();
        doc.setup.facing_pages = true;
        doc.add_page();
        doc.add_page();
        let fourth = doc.add_page();

        assert_eq!(doc.spread_order.len(), 3);
        assert_eq!(doc.pages_of(doc.spread_order[2]), vec![fourth]);
    }

    #[test]
    fn without_facing_pages_every_page_is_its_own_spread() {
        let mut doc = Document::new();
        // A new document faces its pages, so this one has to say otherwise.
        doc.setup.facing_pages = false;
        doc.add_page();
        doc.add_page();
        assert_eq!(doc.spread_order.len(), 3);
    }

    #[test]
    fn the_last_page_cannot_be_removed() {
        let mut doc = Document::new();
        let only = doc.page_ids().next().expect("a page");

        assert!(!doc.remove_page(only), "refused");
        assert_eq!(doc.page_ids().count(), 1);
    }

    #[test]
    fn removing_a_page_takes_what_stood_on_it() {
        let mut doc = Document::new();
        let page = doc.add_page();
        let frame = frame_on(&mut doc, page);

        assert!(doc.remove_page(page));
        assert!(doc.frame(frame).is_none(), "the frame went with the page");
        assert_eq!(doc.page_ids().count(), 1);
    }

    #[test]
    fn removing_a_page_keeps_the_layer_and_what_the_other_pages_hold() {
        // A layer spans the document, so it outlives any one page — and the
        // objects on the *other* pages have to outlive it too. Under the old
        // model the layer was the page's, and taking the page took the layer.
        let mut doc = Document::new();
        let first = doc.page_ids().next().expect("a page");
        let second = doc.add_page();
        let keeper = frame_on(&mut doc, first);
        let victim = frame_on(&mut doc, second);
        let layer = doc.default_layer().expect("a layer");

        assert!(doc.remove_page(second));

        assert!(doc.layers.get(layer).is_some(), "the layer stays");
        assert!(
            doc.frame(keeper).is_some(),
            "so does the other page's frame"
        );
        assert!(doc.frame(victim).is_none());
        assert_eq!(
            doc.layers[layer].frames,
            vec![keeper],
            "and the layer no longer lists what was removed"
        );
    }

    #[test]
    fn removing_a_page_closes_the_gap_it_left() {
        let mut doc = Document::new();
        // One page per spread, so removing one really does leave a gap in the
        // vertical flow rather than a hole beside a survivor.
        doc.setup.facing_pages = false;
        doc.add_page();
        let third = doc.add_page();
        let second = doc.page_ids().nth(1).expect("a second page");

        doc.remove_page(second);

        let first = doc.page_ids().next().expect("a page");
        let a = doc.pages[first].bounds;
        let b = doc.pages[third].bounds;
        assert!(
            (b.y - (a.y + a.height + SPREAD_GAP)).abs() < 1e-9,
            "the page after the hole moved up: {a:?} then {b:?}"
        );
    }

    // --- duplicating and reordering -----------------------------------------

    #[test]
    fn a_duplicated_page_has_its_own_frames() {
        let mut doc = Document::new();
        let page = doc.add_page();
        let frame = frame_on(&mut doc, page);

        let copy = doc.duplicate_page(page).expect("a copy");
        let copied = doc.frames_on_page(copy);

        assert_eq!(copied.len(), 1, "one frame came across");
        assert_ne!(copied[0], frame, "a copy, not the same frame");
        assert!(doc.frame(frame).is_some(), "and the original survives");
    }

    #[test]
    fn a_duplicated_pages_frames_move_onto_the_copy() {
        // The bug the geometric model would have introduced if the copies were
        // left where they were: a frame's page is where it *sits*, so a copy
        // that never moved would still be standing on the original page —
        // doubled content there, and a blank duplicate.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        let page = doc.add_page();
        frame_on(&mut doc, page);

        let copy = doc.duplicate_page(page).expect("a copy");

        assert_eq!(doc.frames_on_page(page).len(), 1, "the original, once");
        assert_eq!(doc.frames_on_page(copy).len(), 1, "the copy, once");
    }

    #[test]
    fn a_duplicated_frame_joins_the_layer_its_original_was_on() {
        let mut doc = Document::new();
        let page = doc.add_page();
        let frame = frame_on(&mut doc, page);
        let layer = doc.layer_of_frame(frame).expect("a layer");

        let copy = doc.duplicate_page(page).expect("a copy");
        let copied = doc.frames_on_page(copy)[0];

        assert_eq!(doc.layer_of_frame(copied), Some(layer));
    }

    #[test]
    fn editing_a_copied_pages_text_leaves_the_original_alone() {
        // The trap a shallow copy sets: both pages look right until somebody
        // types, and then they change together.
        let mut doc = Document::new();
        let page = doc.add_page();
        let story = doc.add_story(Story::new("original"));
        let text = frame_on(&mut doc, page);
        doc.frame_mut(text).expect("frame").kind = FrameKind::text(story);

        let copy = doc.duplicate_page(page).expect("a copy");
        let copied_frame = doc.frames_on_page(copy)[0];
        let FrameKind::Text { story: copied, .. } = doc.frame(copied_frame).expect("frame").kind
        else {
            panic!("a text frame shows a story");
        };

        assert_ne!(copied, story, "its own story");
        doc.story_mut(copied).expect("story").set_text("changed");
        assert_eq!(
            doc.story(story).expect("story").text,
            "original",
            "the original page still says what it said"
        );
    }

    #[test]
    fn a_duplicate_lands_directly_after_its_original() {
        let mut doc = Document::new();
        let first = doc.page_ids().next().expect("a page");
        doc.add_page();

        let copy = doc.duplicate_page(first).expect("a copy");
        assert_eq!(
            doc.page_ids().nth(1),
            Some(copy),
            "the copy is the second page, not the last"
        );
    }

    #[test]
    fn moving_a_spread_changes_the_order_and_the_geometry_follows() {
        let mut doc = Document::new();
        // One page per spread, so the indices being moved are the pages.
        doc.setup.facing_pages = false;
        doc.add_page();
        let third = doc.add_page();

        let before = doc.pages[third].bounds.y;
        doc.move_spread(2, 0);

        assert_eq!(doc.page_ids().next(), Some(third), "it reads first now");
        assert!(
            doc.pages[third].bounds.y < before,
            "and it moved up the pasteboard: {} against {before}",
            doc.pages[third].bounds.y
        );
    }

    #[test]
    fn moving_a_spread_nowhere_does_nothing() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        let order = doc.spread_order.clone();

        doc.move_spread(0, 0);
        doc.move_spread(9, 0);
        doc.move_spread(0, 9);

        assert_eq!(doc.spread_order, order);
    }

    #[test]
    fn every_structural_change_bumps_the_revision() {
        // The resolve cache keys on it. A page that does not appear until
        // something else moves is the margins bug, again.
        let mut doc = Document::new();
        let mut last = doc.revision();
        let moved = |doc: &Document, last: &mut u64, what: &str| {
            assert!(doc.revision() > *last, "{what} did not bump the revision");
            *last = doc.revision();
        };

        let page = doc.add_page();
        moved(&doc, &mut last, "add_page");
        doc.duplicate_page(page);
        moved(&doc, &mut last, "duplicate_page");
        doc.move_spread(1, 0);
        moved(&doc, &mut last, "move_spread");
        doc.remove_page(page);
        moved(&doc, &mut last, "remove_page");
    }

    #[test]
    fn a_new_document_faces_its_pages() {
        // InDesign's own default, and what a layout tool is mostly for.
        assert!(Document::new().setup.facing_pages);
    }

    #[test]
    fn three_pages_in_a_new_document_read_one_then_two_and_three() {
        // Reported from real use: three pages stacked one under another
        // instead of a lone first page and a facing pair. They were each in
        // their own spread, because facing pages defaulted off.
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();

        assert_eq!(doc.spread_order.len(), 2, "a single page, then a pair");
        assert_eq!(doc.pages_of(doc.spread_order[1]).len(), 2);

        let pair = doc.pages_of(doc.spread_order[1]);
        let a = doc.pages[pair[0]].bounds;
        let b = doc.pages[pair[1]].bounds;
        assert!((a.y - b.y).abs() < 1e-9, "level with each other");
        assert!((b.x - (a.x + a.width)).abs() < 1e-9, "and touching");
    }

    #[test]
    fn a_document_written_before_facing_pages_existed_does_not_gain_them() {
        // `DocumentSetup::default()` is what `serde(default)` hands an older
        // file. Turning those into facing-page documents would rearrange
        // somebody's pages on open.
        assert!(!DocumentSetup::default().facing_pages);
    }

    // --- spreads clear one another ------------------------------------------

    /// 10mm, which is what the inspector's bleed defaults to when set.
    const TEN_MM: f64 = 28.346_456_692_913_385;

    #[test]
    fn a_spreads_bleed_does_not_reach_the_next_spread() {
        // Reported from real use: with margins and bleed set, the first page's
        // bleed box ran down into the spread below it. The gap between spreads
        // was a flat 36 points and 10mm of bleed is 28 — 56 points of bleed in
        // a 36 point gap.
        let mut doc = Document::new();
        doc.add_page();
        doc.set_setup(DocumentSetup {
            bleed: Insets::uniform(TEN_MM),
            // From the document's own setup, not from `default()`: the latter
            // would quietly turn facing pages off, which is most of what these
            // tests are about.
            ..doc.setup
        });

        let boxes: Vec<DocRect> = doc.page_ids().filter_map(|p| doc.bleed_rect(p)).collect();

        for (i, a) in boxes.iter().enumerate() {
            for b in boxes.iter().skip(i + 1) {
                let apart = a.y + a.height <= b.y + 1e-9
                    || b.y + b.height <= a.y + 1e-9
                    || a.x + a.width <= b.x + 1e-9
                    || b.x + b.width <= a.x + 1e-9;
                assert!(apart, "two bleed boxes overlap: {a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn a_slug_wider_than_the_bleed_is_what_clears() {
        // Both are measured from the trim, so the clearance is the larger of
        // the two rather than their sum.
        let mut doc = Document::new();
        doc.add_page();
        doc.set_setup(DocumentSetup {
            bleed: Insets::uniform(3.0),
            slug: Insets::uniform(TEN_MM),
            ..doc.setup
        });

        let boxes: Vec<DocRect> = doc.page_ids().filter_map(|p| doc.slug_rect(p)).collect();
        for (i, a) in boxes.iter().enumerate() {
            for b in boxes.iter().skip(i + 1) {
                let apart = a.y + a.height <= b.y + 1e-9
                    || b.y + b.height <= a.y + 1e-9
                    || a.x + a.width <= b.x + 1e-9
                    || b.x + b.width <= a.x + 1e-9;
                assert!(apart, "two slug boxes overlap: {a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn raising_the_bleed_moves_the_spreads_apart() {
        // The bug was as much about *when* as about how far: the setup could
        // change without anything reflowing, so the boxes grew into each other
        // where they stood.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        let second = doc.page_ids().nth(1).expect("a second page");
        let before = doc.pages[second].bounds.y;

        doc.set_setup(DocumentSetup {
            bleed: Insets::uniform(TEN_MM),
            // From the document's own setup, not from `default()`: the latter
            // would quietly turn facing pages off, which is most of what these
            // tests are about.
            ..doc.setup
        });

        assert!(
            doc.pages[second].bounds.y > before,
            "the second spread moved down to make room: {} against {before}",
            doc.pages[second].bounds.y
        );
    }

    #[test]
    fn a_taller_page_moves_the_spread_below_it() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        let second = doc.page_ids().nth(1).expect("a second page");
        let before = doc.pages[second].bounds.y;

        doc.set_page_size(612.0, 1000.0);

        assert!(
            doc.pages[second].bounds.y > before,
            "a taller first page pushes the second down"
        );
    }

    #[test]
    fn two_facing_pages_still_touch_with_a_bleed_set() {
        // The clearance is between *spreads*. Inside one, the pages meet at
        // the fold and their bleed overlapping there is what a fold is.
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();
        doc.set_setup(DocumentSetup {
            bleed: Insets::uniform(TEN_MM),
            // From the document's own setup, not from `default()`: the latter
            // would quietly turn facing pages off, which is most of what these
            // tests are about.
            ..doc.setup
        });

        let pair = doc.pages_of(doc.spread_order[1]);
        let a = doc.pages[pair[0]].bounds;
        let b = doc.pages[pair[1]].bounds;
        assert!(
            (b.x - (a.x + a.width)).abs() < 1e-9,
            "the fold has no gap: {a:?} then {b:?}"
        );
    }

    // --- there is no bleed at a fold ----------------------------------------

    #[test]
    fn facing_pages_do_not_bleed_into_the_fold() {
        // A spread is imposed as one sheet: there is no trim between two
        // facing pages, so there is nothing to bleed past. The ink simply
        // continues across.
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();
        doc.set_setup(DocumentSetup {
            bleed: Insets::uniform(TEN_MM),
            // From the document's own setup, not from `default()`: the latter
            // would quietly turn facing pages off, which is most of what these
            // tests are about.
            ..doc.setup
        });

        let pair = doc.pages_of(doc.spread_order[1]);
        let verso = doc.pages[pair[0]].bounds;
        let recto = doc.pages[pair[1]].bounds;
        let verso_bleed = doc.bleed_rect(pair[0]).expect("a bleed");
        let recto_bleed = doc.bleed_rect(pair[1]).expect("a bleed");

        assert!(
            (verso_bleed.x + verso_bleed.width - (verso.x + verso.width)).abs() < 1e-9,
            "the verso does not bleed past its own right edge, which is the fold"
        );
        assert!(
            (recto_bleed.x - recto.x).abs() < 1e-9,
            "and the recto does not bleed past its left edge"
        );
        assert!(
            (verso.x - verso_bleed.x - TEN_MM).abs() < 1e-9,
            "but it still bleeds on its outside edge"
        );
        assert!(
            (recto_bleed.x + recto_bleed.width - (recto.x + recto.width) - TEN_MM).abs() < 1e-9,
            "and so does the recto"
        );
    }

    #[test]
    fn a_page_that_stands_alone_bleeds_on_all_four_sides() {
        // Page one, and any last page without a partner.
        let mut doc = Document::new();
        doc.add_page();
        doc.set_setup(DocumentSetup {
            bleed: Insets::uniform(TEN_MM),
            // From the document's own setup, not from `default()`: the latter
            // would quietly turn facing pages off, which is most of what these
            // tests are about.
            ..doc.setup
        });

        let first = doc.page_ids().next().expect("a page");
        let bounds = doc.pages[first].bounds;
        let bleed = doc.bleed_rect(first).expect("a bleed");

        assert!((bounds.x - bleed.x - TEN_MM).abs() < 1e-9, "left");
        assert!(
            (bleed.x + bleed.width - (bounds.x + bounds.width) - TEN_MM).abs() < 1e-9,
            "right"
        );
        assert!((bounds.y - bleed.y - TEN_MM).abs() < 1e-9, "top");
        assert!(
            (bleed.y + bleed.height - (bounds.y + bounds.height) - TEN_MM).abs() < 1e-9,
            "bottom"
        );
    }

    #[test]
    fn a_slug_stops_at_a_fold_too() {
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();
        doc.set_setup(DocumentSetup {
            slug: Insets::uniform(TEN_MM),
            ..doc.setup
        });

        let pair = doc.pages_of(doc.spread_order[1]);
        let verso = doc.pages[pair[0]].bounds;
        let slug = doc.slug_rect(pair[0]).expect("a slug");
        assert!(
            (slug.x + slug.width - (verso.x + verso.width)).abs() < 1e-9,
            "no slug at the fold"
        );
    }

    #[test]
    fn a_two_page_spread_bleeds_as_one_sheet() {
        // The two bleed boxes, taken together, are the sheet: they meet at the
        // fold with no gap and no overlap, and reach the bleed distance at
        // either end.
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();
        doc.set_setup(DocumentSetup {
            bleed: Insets::uniform(TEN_MM),
            // From the document's own setup, not from `default()`: the latter
            // would quietly turn facing pages off, which is most of what these
            // tests are about.
            ..doc.setup
        });

        let pair = doc.pages_of(doc.spread_order[1]);
        let left = doc.bleed_rect(pair[0]).expect("a bleed");
        let right = doc.bleed_rect(pair[1]).expect("a bleed");

        assert!(
            (right.x - (left.x + left.width)).abs() < 1e-9,
            "they meet exactly: {left:?} then {right:?}"
        );
        let sheet = left.width + right.width;
        let trim = doc.pages[pair[0]].bounds.width * 2.0;
        assert!(
            (sheet - trim - TEN_MM * 2.0).abs() < 1e-9,
            "and the sheet is the two pages plus one bleed at each end"
        );
    }

    #[test]
    fn a_point_on_the_second_page_is_still_on_a_page() {
        // The cursor asked `first_page_bounds().contains(..)`, which is right
        // only while there is one page: over the spread below it answered
        // "pasteboard" and drew white on white.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        let second = doc.add_page();
        let bounds = doc.pages[second].bounds;

        assert!(doc.on_a_page(DocPoint {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height / 2.0,
        }));
    }

    #[test]
    fn a_point_in_the_gap_between_spreads_is_not_on_a_page() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        let first = doc.page_ids().next().expect("a page");
        let bounds = doc.pages[first].bounds;

        assert!(!doc.on_a_page(DocPoint {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height + 4.0,
        }));
    }

    #[test]
    fn a_spread_owns_its_pages_and_the_room_around_them() {
        let mut doc = Document::new();
        doc.add_page();
        let first = doc.spread_order[0];
        let second = doc.spread_order[1];

        let a = doc.spread_area(first).expect("an area");
        let b = doc.spread_area(second).expect("an area");

        assert!(
            a.y + a.height <= b.y + 1e-9,
            "one spread's area must not reach into the next: {a:?} then {b:?}"
        );
        let page = doc.pages[doc.page_ids().next().expect("a page")].bounds;
        assert!(a.y < page.y && a.x < page.x, "and it reaches past the page");
    }

    #[test]
    fn a_frame_knows_which_spread_it_is_on() {
        let mut doc = Document::new();
        let page = doc.add_page();
        let frame = frame_on(&mut doc, page);

        assert_eq!(doc.spread_of_frame(frame), doc.spread_of(page));
    }

    #[test]
    fn a_frame_drawn_on_the_third_page_is_on_the_third_page() {
        // Reported from real use: an object drawn on page three was clipped to
        // page one's spread and disappeared, because everything joined the
        // first page's layer whatever page it was drawn on. There is no such
        // thing as the first page's layer any more.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        let third = doc.add_page();
        let frame = frame_on(&mut doc, third);

        assert_eq!(doc.page_of_frame(frame), Some(third));
        assert_eq!(doc.spread_of_frame(frame), doc.spread_of(third));
    }

    #[test]
    fn a_point_out_on_the_pasteboard_belongs_to_the_nearest_page() {
        // Somewhere is better than nowhere: a frame beside the page still has
        // to be clipped to *a* spread, or it reaches into the next one.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        let second = doc.page_ids().nth(1).expect("a second page");
        let bounds = doc.pages[second].bounds;

        let page = doc.page_holding(DocPoint {
            x: bounds.x - 200.0,
            y: bounds.y + bounds.height / 2.0,
        });

        assert_eq!(page, Some(second));
    }

    #[test]
    fn dragging_a_frame_to_another_page_needs_no_bookkeeping() {
        // **The whole point of the change.** Moving a frame across the fold
        // used to require `rehome_frame` to move it between layers, and every
        // path that moved a frame and forgot to call it left the frame clipped
        // to the page it came from. There is nothing left to forget.
        use tessera_geometry::Transform;

        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        let second = doc.add_page();
        let first = doc.page_ids().next().expect("a page");
        let frame = frame_on(&mut doc, first);
        let layer = doc.layer_of_frame(frame).expect("a layer");
        assert_eq!(doc.page_of_frame(frame), Some(first));

        let target = doc.pages[second].bounds;
        if let Some(f) = doc.frame_mut(frame) {
            f.transform = Transform::translate(target.x + 20.0, target.y + 20.0);
        }

        assert_eq!(doc.page_of_frame(frame), Some(second), "it moved page");
        assert_eq!(
            doc.layer_of_frame(frame),
            Some(layer),
            "and stayed on its layer, which is what a layer spanning the \
             document means"
        );
        assert_eq!(doc.spread_of_frame(frame), doc.spread_of(second));
    }

    // --- layers span the document -------------------------------------------

    #[test]
    fn a_new_document_has_one_layer_and_it_is_active() {
        let doc = Document::new();
        assert_eq!(doc.layer_ids().count(), 1);
        assert_eq!(doc.active_layer, doc.default_layer());
        assert_eq!(
            doc.layers[doc.default_layer().expect("a layer")].name,
            "Layer 1"
        );
    }

    #[test]
    fn adding_a_page_adds_no_layer() {
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();
        assert_eq!(doc.layer_ids().count(), 1, "one layer, three pages");
    }

    #[test]
    fn one_layer_holds_frames_from_every_page() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        let first = doc.page_ids().next().expect("a page");
        let second = doc.add_page();
        let a = frame_on(&mut doc, first);
        let b = frame_on(&mut doc, second);

        let layer = doc.default_layer().expect("a layer");
        assert_eq!(doc.layers[layer].frames, vec![a, b]);
        assert_eq!(doc.page_of_frame(a), Some(first));
        assert_eq!(doc.page_of_frame(b), Some(second));
    }

    #[test]
    fn a_layer_that_is_hidden_hides_its_frames_on_every_page() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        let first = doc.page_ids().next().expect("a page");
        let second = doc.add_page();
        frame_on(&mut doc, first);
        frame_on(&mut doc, second);

        let layer = doc.default_layer().expect("a layer");
        doc.layers[layer].visible = false;

        assert!(
            doc.paint_order().is_empty(),
            "hiding a document-wide layer hides it everywhere, which is the \
             behaviour a per-page layer could not have"
        );
    }

    #[test]
    fn the_bottom_layer_paints_first() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let under = frame_on(&mut doc, page);

        let over_layer = doc.layers.insert(Layer::named("Layer 2"));
        doc.layer_order.push(over_layer);
        let mut frame = rect_frame();
        frame.bounds = doc.pages[page].bounds;
        let over = doc.add_frame(over_layer, frame);

        assert_eq!(
            doc.paint_order(),
            vec![under, over],
            "layer order decides, not the order the frames were made"
        );
    }

    // --- naming, adding and removing layers ---------------------------------

    #[test]
    fn a_new_layer_goes_on_top_and_becomes_active() {
        let mut doc = Document::new();
        let first = doc.default_layer().expect("a layer");

        let added = doc.add_layer("Guides");

        assert_eq!(
            doc.layer_order,
            vec![first, added],
            "on top, because that is what a new layer is for"
        );
        assert_eq!(doc.active_layer, Some(added));
    }

    #[test]
    fn a_new_layer_is_offered_a_name_no_other_layer_has() {
        let mut doc = Document::new();
        assert_eq!(doc.unused_layer_name(), "Layer 2");
        doc.add_layer(doc.unused_layer_name());
        assert_eq!(doc.unused_layer_name(), "Layer 3");
    }

    #[test]
    fn a_name_freed_by_a_deletion_is_not_offered_over_a_survivor() {
        // Counting layers is not enough. Delete "Layer 1" from a document of
        // two and the count says the next name is "Layer 2" — which the
        // survivor is already called.
        let mut doc = Document::new();
        let first = doc.default_layer().expect("a layer");
        doc.add_layer("Layer 2");

        assert!(doc.remove_layer(first));

        assert_eq!(
            doc.unused_layer_name(),
            "Layer 3",
            "not Layer 2, which still exists"
        );
    }

    #[test]
    fn the_last_layer_cannot_be_removed() {
        let mut doc = Document::new();
        let only = doc.default_layer().expect("a layer");

        assert!(!doc.remove_layer(only), "refused");
        assert_eq!(doc.layer_ids().count(), 1);
    }

    #[test]
    fn removing_a_layer_takes_its_frames_and_leaves_the_others() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let keeper = frame_on(&mut doc, page);
        let doomed_layer = doc.add_layer("Layer 2");
        let doomed = frame_on(&mut doc, page);
        assert_eq!(doc.layer_of_frame(doomed), Some(doomed_layer));

        assert!(doc.remove_layer(doomed_layer));

        assert!(doc.frame(doomed).is_none(), "its frames went with it");
        assert!(doc.frame(keeper).is_some(), "the other layer's did not");
    }

    #[test]
    fn removing_the_active_layer_makes_another_one_active() {
        // Otherwise the next object drawn has nowhere to go.
        let mut doc = Document::new();
        let first = doc.default_layer().expect("a layer");
        let second = doc.add_layer("Layer 2");
        assert_eq!(doc.active_layer, Some(second));

        assert!(doc.remove_layer(second));

        assert_eq!(doc.active_layer, Some(first));
    }

    // --- reordering and moving between them ---------------------------------

    #[test]
    fn reordering_layers_reorders_what_paints_over_what() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let under = frame_on(&mut doc, page);
        doc.add_layer("Layer 2");
        let over = frame_on(&mut doc, page);
        assert_eq!(doc.paint_order(), vec![under, over]);

        doc.move_layer(1, 0);

        assert_eq!(
            doc.paint_order(),
            vec![over, under],
            "the layer that was on top is underneath"
        );
    }

    #[test]
    fn moving_a_frame_between_layers_leaves_it_where_it_is_on_the_page() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let frame = frame_on(&mut doc, page);
        let where_it_was = doc.frame(frame).expect("frame").bounds;
        let up = doc.add_layer("Layer 2");

        doc.move_frames_to_layer(&[frame], up);

        assert_eq!(doc.layer_of_frame(frame), Some(up));
        assert_eq!(doc.page_of_frame(frame), Some(page), "same page");
        assert_eq!(
            doc.frame(frame).expect("frame").bounds,
            where_it_was,
            "changing which layer holds it is not moving it"
        );
    }

    #[test]
    fn moving_a_frame_to_the_layer_it_is_already_on_changes_nothing() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let frame = frame_on(&mut doc, page);
        let layer = doc.layer_of_frame(frame).expect("a layer");
        let before = doc.revision();

        doc.move_frames_to_layer(&[frame], layer);

        assert_eq!(doc.revision(), before);
        assert_eq!(doc.layers[layer].frames, vec![frame], "listed once");
    }

    // --- hiding and locking --------------------------------------------------

    #[test]
    fn a_locked_layers_frames_are_still_drawn() {
        // Locked is not hidden. Keeping a background visible while working over
        // it is the whole use of locking one.
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let frame = frame_on(&mut doc, page);
        let layer = doc.layer_of_frame(frame).expect("a layer");

        doc.layers[layer].locked = true;

        assert_eq!(doc.paint_order(), vec![frame], "drawn");
        assert!(doc.selectable_order().is_empty(), "and not touchable");
    }

    #[test]
    fn a_click_cannot_reach_a_locked_layer() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let frame = frame_on(&mut doc, page);
        let at = doc.frame(frame).expect("frame").bounds.center();
        assert_eq!(doc.hit_test(at, 0.0), Some(frame));

        let layer = doc.layer_of_frame(frame).expect("a layer");
        doc.layers[layer].locked = true;

        assert_eq!(doc.hit_test(at, 0.0), None);
    }

    #[test]
    fn a_click_passes_through_a_locked_layer_to_what_is_under_it() {
        // The behaviour that makes locking useful rather than merely safe.
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let under = frame_on(&mut doc, page);
        let over_layer = doc.add_layer("Layer 2");
        let over = frame_on(&mut doc, page);
        let at = doc.frame(over).expect("frame").bounds.center();
        assert_eq!(doc.hit_test(at, 0.0), Some(over), "the top one, unlocked");

        doc.layers[over_layer].locked = true;

        assert_eq!(doc.hit_test(at, 0.0), Some(under));
    }

    #[test]
    fn a_click_cannot_reach_a_hidden_layer_either() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let frame = frame_on(&mut doc, page);
        let at = doc.frame(frame).expect("frame").bounds.center();
        let layer = doc.layer_of_frame(frame).expect("a layer");

        doc.layers[layer].visible = false;

        assert_eq!(
            doc.hit_test(at, 0.0),
            None,
            "a frame you cannot see but can still catch is worse than one you can"
        );
    }

    #[test]
    fn a_rubber_band_cannot_reach_a_locked_layer() {
        // Reported from real use: the lock stopped a click and not a marquee.
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let frame = frame_on(&mut doc, page);
        let all = doc.pages[page].bounds;
        assert_eq!(doc.frames_touching(all), vec![frame]);

        let layer = doc.layer_of_frame(frame).expect("a layer");
        doc.layers[layer].locked = true;

        assert!(
            doc.frames_touching(all).is_empty(),
            "a lock that a dragged box walks past is worse than none, \
             because it looks like one"
        );
    }

    #[test]
    fn a_rubber_band_cannot_reach_a_hidden_layer_either() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let frame = frame_on(&mut doc, page);
        let all = doc.pages[page].bounds;

        let layer = doc.layer_of_frame(frame).expect("a layer");
        doc.layers[layer].visible = false;

        assert!(doc.frames_touching(all).is_empty());
    }

    #[test]
    fn a_rubber_band_still_catches_what_is_on_an_unlocked_layer_beside_it() {
        // The lock has to be narrow: locking one layer must not make the
        // marquee useless everywhere.
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        let reachable = frame_on(&mut doc, page);
        let locked_layer = doc.add_layer("Layer 2");
        let out_of_reach = frame_on(&mut doc, page);
        doc.layers[locked_layer].locked = true;

        assert_eq!(
            doc.frames_touching(doc.pages[page].bounds),
            vec![reachable],
            "one layer locked, the other still workable"
        );
        assert!(
            doc.paint_order().contains(&out_of_reach),
            "and the locked layer is still drawn"
        );
    }

    // --- which side of the fold a page falls on -----------------------------

    #[test]
    fn page_one_sits_on_the_right_of_its_spread() {
        // A recto is a right-hand page and page one is a recto. Drawn in the
        // left column it reads as the back of a sheet, and the whole document
        // is a page out of step from there on.
        let doc = Document::new();
        assert!(doc.setup.facing_pages, "a new document faces its pages");

        let first = doc.page_ids().next().expect("a page");
        let bounds = doc.pages[first].bounds;
        assert_eq!(
            bounds.x, bounds.width,
            "one page width in: the right-hand column"
        );
    }

    #[test]
    fn a_facing_spread_starts_in_the_left_column() {
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();

        let pages: Vec<_> = doc.page_ids().collect();
        assert_eq!(pages.len(), 3);
        let (left, right) = (doc.pages[pages[1]].bounds, doc.pages[pages[2]].bounds);

        assert_eq!(left.x, 0.0, "page two is a verso");
        assert_eq!(right.x, left.width, "and page three faces it");
        assert_eq!(left.y, right.y, "on one sheet");
    }

    #[test]
    fn a_final_lone_page_sits_on_the_left() {
        // Page four, on its own, is even and so a verso.
        let mut doc = Document::new();
        for _ in 0..3 {
            doc.add_page();
        }
        let pages: Vec<_> = doc.page_ids().collect();
        assert_eq!(pages.len(), 4);

        assert_eq!(doc.pages[pages[3]].bounds.x, 0.0);
    }

    #[test]
    fn pages_that_do_not_face_all_sit_at_the_left() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        doc.reflow_spreads();

        for page in doc.page_ids() {
            assert_eq!(
                doc.pages[page].bounds.x, 0.0,
                "with no spine there is no side to be on"
            );
        }
    }

    // --- content travels with its page --------------------------------------

    #[test]
    fn what_stands_on_a_page_moves_when_the_page_does() {
        // Removing a page slides every page after it upwards. Before this, the
        // contents stayed behind and landed on whichever page arrived beneath
        // them.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        let third = doc.add_page();
        let frame = frame_on(&mut doc, third);
        let offset = {
            let page = doc.pages[third].bounds;
            let on = doc.frame(frame).expect("frame").bounds;
            (on.x - page.x, on.y - page.y)
        };

        let second = doc.page_ids().nth(1).expect("a second page");
        doc.remove_page(second);

        assert_eq!(doc.page_of_frame(frame), Some(third), "still on its page");
        let page = doc.pages[third].bounds;
        let on = doc.frame(frame).expect("frame").bounds;
        assert!(
            ((on.x - page.x) - offset.0).abs() < 1e-9 && ((on.y - page.y) - offset.1).abs() < 1e-9,
            "and in the same place on it"
        );
    }

    #[test]
    fn a_group_travels_with_its_page_whole() {
        // A group's children carry their own geometry, so moving the group's
        // page has to reach all the way down rather than only shifting the
        // group's own box.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let page = doc.page_ids().next().expect("a page");
        let a = frame_on(&mut doc, page);
        let b = frame_on(&mut doc, page);
        let group = doc.group(&[a, b]).expect("a group");
        let (was_a, was_b) = (
            doc.frame(a).expect("frame").bounds,
            doc.frame(b).expect("frame").bounds,
        );

        // Turning facing pages on moves page one across the fold.
        doc.setup.facing_pages = true;
        doc.reflow_spreads();

        let width = doc.pages[page].bounds.width;
        assert_eq!(doc.pages[page].bounds.x, width, "the page moved");
        assert!(doc.frame(group).is_some());
        assert!(
            (doc.frame(a).expect("frame").bounds.x - (was_a.x + width)).abs() < 1e-9
                && (doc.frame(b).expect("frame").bounds.x - (was_b.x + width)).abs() < 1e-9,
            "and both children went with it, not just the group's box"
        );
    }

    // --- moving one page ----------------------------------------------------

    // --- the sequence decides the spreads -----------------------------------

    /// "1 | 2-3 | 4-5", as the spreads currently stand.
    fn pagination(doc: &Document) -> String {
        let all: Vec<_> = doc.page_ids().collect();
        let number = |p: &PageId| all.iter().position(|x| x == p).map_or(0, |i| i + 1);
        doc.spread_order
            .iter()
            .map(|s| {
                doc.pages_of(*s)
                    .iter()
                    .map(|p| number(p).to_string())
                    .collect::<Vec<_>>()
                    .join("-")
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn facing_pages_pack_one_then_pairs() {
        let mut doc = Document::new();
        for _ in 0..5 {
            doc.add_page();
        }
        assert_eq!(pagination(&doc), "1 | 2-3 | 4-5 | 6");
    }

    #[test]
    fn pages_that_do_not_face_get_one_spread_each() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.add_page();
        doc.add_page();
        assert_eq!(pagination(&doc), "1 | 2 | 3");
    }

    #[test]
    fn a_moved_page_lands_where_it_was_dropped() {
        let mut doc = Document::new();
        for _ in 0..3 {
            doc.add_page();
        }
        let pages: Vec<_> = doc.page_ids().collect();
        let first = pages[0];

        // Page one to the end.
        assert!(doc.move_page(first, 3));

        let after: Vec<_> = doc.page_ids().collect();
        assert_eq!(after.iter().position(|p| *p == first), Some(3));
    }

    #[test]
    fn moving_a_page_repacks_the_spreads_around_it() {
        // Reported from real use: moving a page left a spread holding two
        // pages whose first was odd-numbered — recto says right-hand column,
        // two pages need both columns, so both drew on the right and the left
        // column could not be dropped onto at all.
        let mut doc = Document::new();
        for _ in 0..4 {
            doc.add_page();
        }
        assert_eq!(pagination(&doc), "1 | 2-3 | 4-5");

        let last = doc.page_ids().last().expect("a page");
        assert!(doc.move_page(last, 0));

        assert_eq!(
            pagination(&doc),
            "1 | 2-3 | 4-5",
            "the same shape, whatever moved through it"
        );
    }

    #[test]
    fn no_spread_ever_holds_two_pages_starting_on_a_recto() {
        // The invariant the bug broke, checked over every move rather than at
        // one place: a spread of two must begin on a verso.
        let mut doc = Document::new();
        for _ in 0..5 {
            doc.add_page();
        }

        for from in 0..6 {
            for to in 0..6 {
                let mut doc = doc.clone();
                let page = doc.page_ids().nth(from).expect("a page");
                doc.move_page(page, to);

                for spread in doc.spread_order.clone() {
                    if doc.pages_of(spread).len() > 1 {
                        assert!(
                            !doc.starts_on_a_recto(spread),
                            "moving {from} to {to} left {}",
                            pagination(&doc)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_page_sits_in_one_of_two_columns() {
        // The other half of the same invariant: a page drawn in column two or
        // beyond has fallen off the sheet, which is what the empty left-hand
        // gap actually was.
        let mut doc = Document::new();
        for _ in 0..5 {
            doc.add_page();
        }
        let width = doc.first_page_bounds().width;

        let page = doc.page_ids().nth(4).expect("a page");
        doc.move_page(page, 0);

        for id in doc.page_ids() {
            let column = (doc.pages[id].bounds.x / width).round() as i32;
            assert!(
                (0..=1).contains(&column),
                "page in column {column}, which is off the sheet"
            );
        }
    }

    #[test]
    fn removing_a_page_repacks_what_is_left() {
        // Taking page two out of 1 | 2-3 | 4-5 has to give 1 | 3-4 | 5, not
        // 1 | 3 | 4-5 — otherwise every page after the hole changes sides.
        let mut doc = Document::new();
        for _ in 0..4 {
            doc.add_page();
        }
        let second = doc.page_ids().nth(1).expect("a second page");

        doc.remove_page(second);

        assert_eq!(pagination(&doc), "1 | 2-3 | 4");
    }

    #[test]
    fn moving_a_page_where_it_already_is_changes_nothing() {
        let mut doc = Document::new();
        doc.add_page();
        let page = doc.page_ids().next().expect("a page");
        let before = doc.revision();

        assert!(!doc.move_page(page, 0));

        assert_eq!(doc.revision(), before);
    }

    #[test]
    fn a_moved_page_takes_what_stands_on_it() {
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();
        let travelling = doc.page_ids().nth(2).expect("a third page");
        let frame = frame_on(&mut doc, travelling);
        let offset = {
            let page = doc.pages[travelling].bounds;
            let on = doc.frame(frame).expect("frame").bounds;
            (on.x - page.x, on.y - page.y)
        };

        assert!(doc.move_page(travelling, 0));

        assert_eq!(doc.page_of_frame(frame), Some(travelling));
        let page = doc.pages[travelling].bounds;
        let on = doc.frame(frame).expect("frame").bounds;
        assert!(
            ((on.x - page.x) - offset.0).abs() < 1e-9 && ((on.y - page.y) - offset.1).abs() < 1e-9,
            "in the same place on the page it was on"
        );
    }

    #[test]
    fn guides_stay_on_the_spread_they_were_dragged_onto() {
        // Guides belong to a spread, and repacking reuses spread objects in
        // order rather than rebuilding them, so a guide does not follow
        // whichever page happens to land there next.
        let mut doc = Document::new();
        for _ in 0..3 {
            doc.add_page();
        }
        let second = doc.spread_order[1];
        doc.add_guide(
            second,
            Guide {
                axis: Axis::Vertical,
                position: 100.0,
                locked: false,
            },
        );

        let last = doc.page_ids().last().expect("a page");
        doc.move_page(last, 0);

        assert_eq!(doc.spread_order.get(1).copied(), Some(second));
        assert_eq!(doc.guides_of(second).len(), 1);
    }

    // --- parent pages -------------------------------------------------------

    /// A document whose pages do not face, with a one-page master carrying a
    /// single item. Returns the document, the master and its item.
    ///
    /// Non-facing on purpose: a facing master has a verso and a recto, and
    /// which of them a page inherits is a separate question with a test of its
    /// own. Mixing the two makes every inheritance test depend on page parity,
    /// which is how the first draft of these came to assert nothing.
    fn a_master_holding_one_item() -> (Document, MasterId, FrameId) {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let item = frame_on(&mut doc, on);
        (doc, master, item)
    }

    #[test]
    fn a_master_is_shaped_like_the_document() {
        // A master exists to be applied to document pages. One shaped
        // differently could not be.
        let mut doc = Document::new();
        let master = doc.add_master("A-Master");

        let pages = doc.pages_of_master(master);
        assert_eq!(pages.len(), 2, "facing pages, so a facing master");
        let page = doc.pages[pages[0]].bounds;
        assert_eq!(page.width, doc.first_page_bounds().width);
        assert_eq!(page.height, doc.first_page_bounds().height);
    }

    #[test]
    fn a_master_for_pages_that_do_not_face_is_one_page() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        let master = doc.add_master("A-Master");
        assert_eq!(doc.pages_of_master(master).len(), 1);
    }

    #[test]
    fn a_master_page_is_not_a_document_page() {
        // It is never numbered, never reflows with the document, and never
        // appears in the reading order.
        let mut doc = Document::new();
        let before: Vec<_> = doc.page_ids().collect();
        let master = doc.add_master("A-Master");

        assert_eq!(doc.page_ids().collect::<Vec<_>>(), before);
        for page in doc.pages_of_master(master) {
            assert!(doc.is_master_page(page));
            assert!(!doc.page_ids().any(|p| p == page));
        }
    }

    #[test]
    fn a_frame_drawn_on_a_master_belongs_to_the_master_page() {
        // `page_holding` asks every page, not only the reading order. Asking
        // only the reading order would hand a master's contents to whichever
        // document page happened to be nearest.
        let mut doc = Document::new();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let frame = frame_on(&mut doc, on);

        assert_eq!(doc.page_of_frame(frame), Some(on));
        assert_eq!(doc.frames_on_page(on), vec![frame]);
    }

    #[test]
    fn a_master_is_offered_a_name_no_other_master_has() {
        let mut doc = Document::new();
        assert_eq!(doc.unused_master_name(), "A-Master");
        doc.add_master(doc.unused_master_name());
        assert_eq!(doc.unused_master_name(), "B-Master");
    }

    // --- applying one -------------------------------------------------------

    #[test]
    fn a_page_shows_what_its_master_holds() {
        let (mut doc, master, item) = a_master_holding_one_item();

        let page = doc.page_ids().next().expect("a page");
        assert!(doc.apply_master(page, Some(master)));

        let inherited = doc.inherited_by(page);
        assert_eq!(inherited.len(), 1);
        assert_eq!(inherited[0].0, item, "the master's own frame, not a copy");
    }

    #[test]
    fn applying_a_master_copies_nothing() {
        // **The whole point.** A master whose items were copied onto each page
        // would not update those pages when it changed.
        let (mut doc, master, item) = a_master_holding_one_item();
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));
        let frames_before = doc.frames.len();

        // Edit the master item.
        doc.frame_mut(item).expect("frame").fill = Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };

        assert_eq!(doc.frames.len(), frames_before, "no copy was ever made");
        let (id, _, _) = doc.inherited_by(page)[0];
        assert_eq!(
            doc.frame(id).expect("frame").fill,
            Color::Rgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0
            },
            "so the page shows the change"
        );
    }

    #[test]
    fn one_master_reaches_every_page_it_is_applied_to() {
        let (mut doc, master, _) = a_master_holding_one_item();
        doc.add_page();
        doc.add_page();

        for page in doc.page_ids().collect::<Vec<_>>() {
            doc.apply_master(page, Some(master));
        }

        assert_eq!(doc.page_ids().count(), 3);
        for page in doc.page_ids() {
            assert_eq!(doc.inherited_by(page).len(), 1, "on every one of them");
        }
    }

    #[test]
    fn a_master_item_lands_in_the_same_place_on_the_page() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let item = frame_on(&mut doc, on);

        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));

        let (_, dx, dy) = doc.inherited_by(page)[0];
        let (from, to) = (doc.pages[on].bounds, doc.pages[page].bounds);
        let was = doc.frame(item).expect("frame").bounds;
        assert!(
            ((was.x + dx) - (to.x + (was.x - from.x))).abs() < 1e-9
                && ((was.y + dy) - (to.y + (was.y - from.y))).abs() < 1e-9,
            "the offset carries it to the same spot on the page"
        );
    }

    #[test]
    fn a_page_takes_the_master_page_on_its_own_side_of_the_fold() {
        // A master with different inside and outside margins is useless if a
        // verso takes the recto's furniture.
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page();
        let master = doc.add_master("A-Master");
        let (verso, recto) = {
            let pages = doc.pages_of_master(master);
            (pages[0], pages[1])
        };

        let pages: Vec<_> = doc.page_ids().collect();
        // Page one is a recto, page two a verso.
        doc.apply_master(pages[0], Some(master));
        doc.apply_master(pages[1], Some(master));

        assert_eq!(
            doc.pages[pages[0]].master,
            Some(recto),
            "page one is a recto"
        );
        assert_eq!(doc.pages[pages[1]].master, Some(verso), "page two a verso");
    }

    #[test]
    fn a_master_cannot_be_applied_to_a_master_page() {
        let mut doc = Document::new();
        let a = doc.add_master("A-Master");
        let b = doc.add_master("B-Master");
        let page = doc.pages_of_master(b)[0];

        assert!(!doc.apply_master(page, Some(a)));
    }

    #[test]
    fn a_duplicated_page_keeps_its_master() {
        let mut doc = Document::new();
        let master = doc.add_master("A-Master");
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));

        let copy = doc.duplicate_page(page).expect("a copy");

        assert_eq!(doc.pages[copy].master, doc.pages[page].master);
    }

    // --- overriding one item ------------------------------------------------

    #[test]
    fn overriding_an_item_leaves_the_page_looking_the_same() {
        // Overriding changes nothing until the copy is edited, which is the
        // whole point: it is a promotion, not an edit.
        let (mut doc, master, item) = a_master_holding_one_item();
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));

        let (_, dx, dy) = doc.inherited_by(page)[0];
        let was = doc.frame(item).expect("frame").bounds;

        let local = doc.override_master_item(page, item).expect("a local copy");

        let now = doc.frame(local).expect("frame").bounds;
        assert!(
            (now.x - (was.x + dx)).abs() < 1e-9 && (now.y - (was.y + dy)).abs() < 1e-9,
            "it sits exactly where the master item appeared"
        );
    }

    #[test]
    fn an_overridden_item_is_drawn_once_not_twice() {
        let (mut doc, master, item) = a_master_holding_one_item();
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));

        doc.override_master_item(page, item);

        assert!(
            doc.inherited_by(page).is_empty(),
            "the master's copy is suppressed where the local one stands"
        );
        assert_eq!(
            doc.frames_on_page(page).len(),
            1,
            "and the local one is there"
        );
    }

    #[test]
    fn overriding_on_one_page_leaves_the_others_alone() {
        // The sentence milestone 3 has to perform.
        let (mut doc, master, item) = a_master_holding_one_item();
        doc.add_page();
        doc.add_page();
        let pages: Vec<_> = doc.page_ids().collect();
        for page in &pages {
            doc.apply_master(*page, Some(master));
        }

        doc.override_master_item(pages[1], item);

        assert!(doc.inherited_by(pages[1]).is_empty(), "overridden here");
        for page in [pages[0], pages[2]] {
            assert_eq!(
                doc.inherited_by(page).len(),
                1,
                "and untouched everywhere else"
            );
        }
    }

    #[test]
    fn an_overridden_item_can_be_edited_without_changing_the_master() {
        let (mut doc, master, item) = a_master_holding_one_item();
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));
        let local = doc.override_master_item(page, item).expect("a copy");

        doc.frame_mut(local).expect("frame").fill = Color::Rgb {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        };

        assert_ne!(
            doc.frame(item).expect("frame").fill,
            Color::Rgb {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0
            },
            "the master is not the copy"
        );
    }

    #[test]
    fn removing_overrides_lets_the_master_show_through_again() {
        let (mut doc, master, item) = a_master_holding_one_item();
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));
        let local = doc.override_master_item(page, item).expect("a copy");

        assert_eq!(doc.remove_overrides(page), 1);

        assert!(doc.frame(local).is_none(), "the local copy went");
        assert_eq!(doc.inherited_by(page).len(), 1, "the master is back");
    }

    #[test]
    fn a_page_with_nothing_overridden_has_nothing_to_remove() {
        let mut doc = Document::new();
        let page = doc.page_ids().next().expect("a page");
        assert_eq!(doc.remove_overrides(page), 0);
    }

    // --- removing a master --------------------------------------------------

    #[test]
    fn removing_a_master_unhooks_the_pages_that_used_it() {
        let (mut doc, master, _) = a_master_holding_one_item();
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));

        assert!(doc.remove_master(master));

        assert_eq!(doc.pages[page].master, None);
        assert!(doc.inherited_by(page).is_empty());
        assert!(!doc.master_ids().any(|m| m == master));
    }

    #[test]
    fn removing_a_master_keeps_what_was_overridden_from_it() {
        // An override is an ordinary frame by then, and deleting somebody's
        // work because a master went is a surprise no undo should have to fix.
        let (mut doc, master, item) = a_master_holding_one_item();
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));
        let local = doc.override_master_item(page, item).expect("a copy");

        doc.remove_master(master);

        assert!(doc.frame(local).is_some(), "the local copy stays");
    }
}
