//! Resolves a document into drawable items.
//!
//! Both the screen renderer and the PDF writer consume the output of this
//! module, so neither re-derives geometry nor re-shapes text. That shared
//! source is what keeps an export from drifting away from the screen.

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::ids::{FrameId, PageId, StoryId};
use tessera_document::nodes::{FrameKind, Stroke};
use tessera_document::paint::Paint;
use tessera_document::path::fit_to_bounds;
use tessera_geometry::{DocRect, Transform};
use tessera_text::shape::{ShapedText, Shaper};
use tessera_text::story::Story as TextStory;
use tessera_text::variables::Variables;

pub use crate::running::{OnPage, Running};

pub use tessera_document::document::StoryMap;

#[derive(Debug, Clone)]
pub enum ResolvedKind {
    Rectangle {
        fill: Paint,
        stroke: Option<Stroke>,
        /// The outline, when the corners are cut.
        ///
        /// Resolved once here rather than built by the renderer and again by
        /// the PDF writer. A rounded corner computed twice is two corners that
        /// agree until somebody fixes a rounding error in one of them.
        outline: Option<kurbo::BezPath>,
    },
    Ellipse {
        fill: Paint,
        stroke: Option<Stroke>,
    },
    Text {
        shaped: ShapedText,
        color: Color,
        /// Lines this frame could not fit.
        ///
        /// **Handed out rather than recomputed.** The flow pass is the only
        /// thing that knows: it alone accounts for where the frame starts in a
        /// thread, its columns, the objects the text runs around and the
        /// baseline grid. Anyone measuring the whole story against one frame's
        /// height gets a different — and wrong — answer, which is what the
        /// overset mark used to be drawn from.
        ///
        /// Non-zero on every frame of a thread except the last, because
        /// passing text on is what the rest of the chain is for. Whether that
        /// counts as *overset* is the caller's question, not this one's.
        overset_lines: usize,
    },
    /// A table, laid out in frame-local coordinates.
    ///
    /// Carries the whole grid rather than one item per cell so that a consumer
    /// draws the rules once, from the edges, instead of four times per cell —
    /// which is what puts a double-weight line between every pair of them.
    Table {
        laid: crate::table::LaidTable,
        /// The rule drawn between and around the cells.
        stroke: Option<Stroke>,
    },
    /// A path in frame-local coordinates. Consumers translate by
    /// [`ResolvedItem::bounds`]'s origin.
    Path {
        path: kurbo::BezPath,
        fill: Option<Paint>,
        stroke: Option<Stroke>,
        /// The story this path carries, set along it — type on a path — with
        /// the colour it is drawn in. In the path's own coordinates, like the
        /// path.
        text: Option<(crate::path_text::PlacedPathText, Color)>,
    },
    /// A container showing artwork, or waiting for some.
    ///
    /// The **path to the file** rather than its pixels: decoding belongs to
    /// the renderer, which can cache what it decodes, and the PDF writer wants
    /// the bytes rather than a decoded surface. Handing both a decoded image
    /// would decode twice and cache neither.
    Graphic {
        /// Where the artwork sits inside the frame, in the frame's own space.
        inner: Transform,
        /// The file, when there is one and it is on disk.
        source: Option<std::path::PathBuf>,
        /// What the artwork wants to be, in points.
        natural: (f64, f64),
        /// Whether the frame is empty, or its file has gone.
        ///
        /// Both draw the placeholder, and the difference matters to the links
        /// panel rather than to the renderer.
        missing: bool,
        stroke: Option<Stroke>,
    },
}

/// A hyperlink, placed: the rectangles its text covers and where it goes.
///
/// Rectangles are in the frame's own space, before its transform, like the
/// glyphs; whoever writes them applies the item's transform as for ink.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLink {
    pub rects: Vec<DocRect>,
    pub target: LinkTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    Url(String),
    /// The page's index in the reading order.
    Page(usize),
}

#[derive(Debug, Clone)]
pub struct ResolvedItem {
    pub frame: FrameId,
    /// The hyperlinks in this item's text, with the rectangles they cover.
    /// Empty for anything that is not text or has no link.
    pub links: Vec<ResolvedLink>,
    /// The page this item was resolved for.
    ///
    /// Not always the page the frame stands on: a parent's item is resolved
    /// once per page that inherits it, and this is which. It is what makes
    /// one master folio read "12" on page 12 and "13" on page 13, and what
    /// lets an object anchored in a parent's text find the same page.
    pub on: Option<PageId>,
    pub bounds: DocRect,
    /// The frame's own space, mapped onto the document. Both the renderer
    /// and the PDF writer apply this the same way, from this one value.
    pub transform: Transform,
    /// The area this frame's spread owns.
    ///
    /// A frame may hang off its page — that is what a pasteboard is for — but
    /// it may not reach into the next spread, which is a different sheet of
    /// paper. Carried per item because the renderer walks a flat list and has
    /// no other way to know which sheet it is drawing on.
    pub spread_area: Option<DocRect>,
    /// How this object composites onto what is behind it.
    ///
    /// Carried on the item rather than inside `kind`, because it applies to the
    /// whole object whatever the object is — which is exactly what makes it a
    /// different fact from a fill colour's alpha. Putting it on each kind would
    /// be four copies of one property and an invitation to forget one.
    pub blend: tessera_document::blending::Blending,
    /// The shadow this object casts, if any.
    ///
    /// On the item beside `blend` rather than inside `kind`, for the same
    /// reason: every kind of object can cast one, and four copies of the field
    /// would be an invitation to forget one.
    pub shadow: Option<tessera_document::shadow::Shadow>,
    pub kind: ResolvedKind,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedDocument {
    /// Back to front. The last item paints on top.
    pub items: Vec<ResolvedItem>,
    /// Every page, with the rectangles that describe it.
    ///
    /// Computed once here so that the screen and the PDF cannot disagree
    /// about where the trim is. While each computed its own, one of them was
    /// eventually going to be wrong.
    pub pages: Vec<ResolvedPage>,
    /// The headings the contents recipe names, with the page each begins
    /// on, in reading order — what a PDF reader shows in its outline pane.
    /// Empty when the document has no contents recipe.
    pub bookmarks: Vec<Bookmark>,
}

/// A heading and where it is, for a reader's outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    /// The page's index in the reading order.
    pub page: usize,
    /// How deep: the heading's level in the contents recipe, 0 first.
    pub level: usize,
}

/// One page, with the rectangles that describe it.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPage {
    /// The trim: the paper itself.
    pub bounds: DocRect,
    /// The type area, inset by the margins.
    pub margins: DocRect,
    /// The trim plus its bleed.
    pub bleed: DocRect,
    /// The trim plus its slug.
    pub slug: DocRect,
    /// The page's column guides. Empty for a single column.
    pub columns: Vec<DocRect>,
}

/// What a resolve is looking at.
///
/// A parent page is edited **in isolation**, so the canvas shows either the
/// document or one parent, never both. Passing the choice in rather than
/// filtering afterwards means the PDF writer cannot accidentally be handed a
/// parent: export asks for [`Scope::Document`] and gets the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The reading order, with each page showing what it inherits.
    Document,
    /// One parent spread, on its own.
    Master(tessera_document::ids::MasterId),
}

/// Resolve every visible frame of the document, in paint order.
pub fn resolve(doc: &Document, shaper: &mut Shaper) -> ResolvedDocument {
    resolve_scope(doc, shaper, Scope::Document)
}

/// Text an input method is composing, to be laid out but not stored.
///
/// A request rather than a story: the splice is made once inside
/// [`resolve_composing`], so every frame threaded through the same story sees
/// the same text — including the frames *before* the one being typed in, whose
/// job is to say how far into the story this one starts. Splicing per frame
/// would let those two disagree, and threaded text would jump by the length of
/// the composition on the frame the caret is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composing {
    pub story: StoryId,
    /// What it stands in for, as a byte range of the stored story.
    ///
    /// Empty at a bare caret. A composition begun with text selected replaces
    /// that text, which is what committing does — so the preview has to show
    /// the same thing, or it is showing a result that will not happen.
    pub replacing: std::ops::Range<usize>,
    pub text: String,
}

/// Resolve what `scope` is looking at.
pub fn resolve_scope(doc: &Document, shaper: &mut Shaper, scope: Scope) -> ResolvedDocument {
    resolve_composing(doc, shaper, scope, None)
}

/// The same, showing text an input method has not committed yet.
pub fn resolve_composing(
    doc: &Document,
    shaper: &mut Shaper,
    scope: Scope,
    composing: Option<&Composing>,
) -> ResolvedDocument {
    let shown: Vec<tessera_document::ids::PageId> = match scope {
        Scope::Document => doc.page_ids().collect(),
        Scope::Master(id) => doc.pages_of_master(id),
    };
    // Spliced once, here, and lent to every frame below.
    let spliced = composing.and_then(|c| {
        Some((
            c.story,
            doc.story(c.story)?
                .with_provisional(c.replacing.clone(), &c.text),
        ))
    });
    let composed = spliced.as_ref().map(|(id, story)| (*id, story));
    resolve_pages(doc, shaper, &shown, composed)
}

/// The story to lay out: the document's own, unless something is being composed
/// into it.
///
/// One function, so that every place needing a story asks the same question. Two
/// places reading `doc.story` directly is how the frame being typed in came to
/// show the composition while the frame before it in the thread did not.
fn story_of<'a>(
    doc: &'a Document,
    composed: Option<(StoryId, &'a TextStory)>,
    id: StoryId,
) -> Option<&'a TextStory> {
    match composed {
        Some((composing, story)) if composing == id => Some(story),
        _ => doc.story(id),
    }
}

/// Resolve exactly these pages, and what they inherit.
///
/// The scope has already been turned into a list of pages by the time this
/// runs, which is the whole of the difference between looking at the document
/// and looking at one parent.
fn resolve_pages<'a>(
    doc: &'a Document,
    shaper: &mut Shaper,
    shown: &[tessera_document::ids::PageId],
    composed: Option<(StoryId, &'a TextStory)>,
) -> ResolvedDocument {
    let pages = shown
        .iter()
        .copied()
        .filter_map(|id| {
            Some(ResolvedPage {
                bounds: doc.pages.get(id)?.bounds,
                margins: doc.margin_rect(id)?,
                bleed: doc.bleed_rect(id)?,
                slug: doc.slug_rect(id)?,
                columns: doc.column_rects(id),
            })
        })
        .collect();

    // Everything standing on a page this scope is showing. A parent's own
    // items are drawn when the parent is what is being looked at, and only
    // then — otherwise a parent spread would sit in the scroll a person is
    // trying to lay out in.
    //
    // **Resolved before the parent items, though painted after them.** A
    // running header on a parent reads the headings on the page, and what is
    // on the page is not known until the page's own text has been laid out.
    // Nothing on a page reads a running header of its own page's content —
    // a body frame carrying one reads as nothing — which is what keeps this
    // from being circular.
    let mut own = Vec::new();
    let running = Running::default();
    for id in doc.paint_order() {
        let Some(on) = doc.page_of_frame(id) else {
            continue;
        };
        if !shown.contains(&on) {
            continue;
        }
        let Some(frame) = doc.frame(id) else { continue };
        // An anchored frame is placed by the text it sits in, below. Its own
        // bounds say how big it is and nothing about where it goes, so drawing
        // it here would put it wherever it was last left.
        if frame.anchor.is_some() {
            continue;
        }
        if let Some(item) = resolve_one(doc, shaper, id, frame, composed, on, &running) {
            own.push(item);
        }
    }

    // What each page's headings say, read off the layout just made — and
    // where each text anchor landed.
    let mut running = Running::read(doc, &own);

    // **A second pass, when the body refers to itself.** A cross-reference
    // in a body frame reads as the page its anchor is on, and that is not
    // known until the pages are laid out — so they are laid out again with
    // the answers. A reference whose text grew may move its own anchor a
    // page; the next relayout says the right thing, as InDesign's stale
    // references do until updated, and a third pass here would not end
    // that in every case either.
    if doc.stories.values().any(|s| !s.cross_references.is_empty()) {
        let mut again = Vec::new();
        for id in doc.paint_order() {
            let Some(on) = doc.page_of_frame(id) else {
                continue;
            };
            if !shown.contains(&on) {
                continue;
            }
            let Some(frame) = doc.frame(id) else { continue };
            if frame.anchor.is_some() {
                continue;
            }
            if let Some(item) = resolve_one(doc, shaper, id, frame, composed, on, &running) {
                again.push(item);
            }
        }
        own = again;
        running = Running::read(doc, &own);
    }

    let mut items = Vec::new();

    // What each page inherits from its parent, drawn **behind** its own
    // contents and so before them: a master carries the furniture a page is
    // laid out on top of.
    //
    // The offset is applied to the resolved item rather than to the frame,
    // because nothing is moved. One master item is drawn once per page that
    // inherits it, from a single frame — copying it onto each page is the
    // thing a master exists in order not to do. What *is* per page is what
    // the text says: the item is resolved once per inheriting page, with that
    // page's number, so one master folio reads "12" on page 12.
    for page in shown.iter().copied() {
        let Some(area) = doc.spread_of(page).and_then(|s| doc.spread_area(s)) else {
            continue;
        };
        for (item, dx, dy) in doc.inherited_by(page) {
            if !doc
                .layer_of_frame(item)
                .and_then(|id| doc.layers.get(id))
                .is_some_and(|layer| layer.visible)
            {
                continue;
            }
            for leaf in doc.descendants(item) {
                let Some(frame) = doc.frame(leaf) else {
                    continue;
                };
                let Some(mut resolved) =
                    resolve_one(doc, shaper, leaf, frame, composed, page, &running)
                else {
                    continue;
                };
                resolved.transform = resolved.transform.then(Transform::translate(dx, dy));
                resolved.spread_area = Some(area);
                items.push(resolved);
            }
        }
    }

    items.extend(own);

    // After the hosts, because where an anchored object lands is not known
    // until the text around it has been broken into lines.
    let anchored = resolve_anchored(doc, shaper, composed, &items, &running);
    items.extend(anchored);

    let mut resolved = ResolvedDocument {
        items,
        pages,
        bookmarks: Vec::new(),
    };
    // The outline: the contents recipe's headings, read off the layout just
    // made. Only for the document — a parent has no reading order.
    if shown.len() == doc.page_ids().count() && !doc.contents.levels.is_empty() {
        let styles: Vec<tessera_text::story::ParagraphStyleId> =
            doc.contents.levels.iter().map(|l| l.style).collect();
        let pages: Vec<PageId> = doc.page_ids().collect();
        resolved.bookmarks = crate::contents::headings(doc, &resolved, &styles)
            .into_iter()
            .filter_map(|h| {
                Some(Bookmark {
                    title: h.text,
                    page: pages.iter().position(|p| *p == h.page)?,
                    level: styles.iter().position(|s| *s == h.style)?,
                })
            })
            .collect();
    }
    resolved
}

/// Resolve the frames anchored in text that has already been laid out.
///
/// **A second pass, after the hosts.** An anchored frame has no position of its
/// own — where it lands is wherever its marker ended up, which is not known
/// until the text around it has been broken into lines. So the hosts resolve
/// first, and this reads the answer off them.
///
/// Nothing is drawn twice: an anchored frame is skipped by the ordinary walk,
/// because the position it would be drawn at there is meaningless.
fn resolve_anchored(
    doc: &Document,
    shaper: &mut Shaper,
    composed: Option<(StoryId, &TextStory)>,
    hosts: &[ResolvedItem],
    running: &Running,
) -> Vec<ResolvedItem> {
    let mut out = Vec::new();

    for host in hosts {
        let ResolvedKind::Text { shaped, .. } = &host.kind else {
            continue;
        };
        let Some(FrameKind::Text { story, .. }) = doc.frame(host.frame).map(|f| &f.kind) else {
            continue;
        };
        let Some(text) = doc.story(*story) else {
            continue;
        };
        let markers = tessera_document::anchored::marker_offsets(&text.text);
        if markers.is_empty() {
            continue;
        }
        let anchors = doc.anchors_in(*story);

        for placed in shaped.lines.iter().flat_map(|line| &line.objects) {
            // The marker's index is what names the frame; the offset is only
            // how the shaper reported it back.
            let Some(index) = markers.iter().position(|at| *at == placed.at) else {
                continue;
            };
            let Some(id) = anchors.frame_at(index) else {
                continue;
            };
            let Some(frame) = doc.frame(id) else { continue };
            // On whichever page its host was resolved for: an object anchored
            // in a parent's text is drawn on every page inheriting it.
            let Some(on) = host.on else { continue };
            let Some(mut item) = resolve_one(doc, shaper, id, frame, composed, on, running) else {
                continue;
            };

            // From the frame's own origin to where the line put it, then
            // through the host's transform — so an anchored picture in a
            // rotated text frame rotates with it, which is what "anchored"
            // has to mean or the two come apart.
            let shift = Transform::translate(
                host.bounds.x + placed.x - frame.bounds.x,
                host.bounds.y + placed.y
                    - frame.bounds.y
                    - frame.anchor.map(|a| a.baseline_shift).unwrap_or(0.0),
            );
            item.transform = shift.then(host.transform);
            item.spread_area = host.spread_area;
            out.push(item);
        }
    }
    out
}

/// A stroke with its colour resolved.
///
/// Every colour leaving `resolve` is a value rather than a name, so the
/// renderer and the PDF writer never meet a swatch — which is what lets them
/// stay ignorant of the document entirely.
fn resolved_stroke(doc: &Document, stroke: Option<&Stroke>) -> Option<Stroke> {
    stroke.map(|s| Stroke {
        color: doc.resolve_colour(&s.color),
        ..s.clone()
    })
}

/// The objects a frame's text must run around, in the text's own space.
///
/// Only the frames that say they wrap, only on the same spread, and never the
/// frame itself — an object cannot push its own text aside. Frames without a
/// wrap are invisible to the text, which is what makes the default free.
///
/// A rotated obstacle is taken by its upright bounding box. A wrap is a
/// horizontal run per line, so an angled outline has to be reduced to a
/// rectangle somewhere; doing it here keeps the shaper honest about what it
/// was given.
fn obstacles_for(
    doc: &Document,
    id: FrameId,
    frame: &tessera_document::nodes::Frame,
    measure: f64,
) -> Vec<tessera_text::wrap::Obstacle> {
    let Some(spread) = doc.spread_of_frame(id) else {
        return Vec::new();
    };
    let Some(mine) = doc.visual_bounds(id) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for page in doc.pages_of(spread) {
        for other in doc.frames_on_page(page) {
            if other == id {
                continue;
            }
            let Some(wrap) = doc
                .frame(other)
                .map(|f| f.wrap)
                .filter(|w| w.standoff().is_some())
            else {
                continue;
            };
            let standoff = wrap.standoff().unwrap_or_default();
            let Some(bounds) = doc.visual_bounds(other) else {
                continue;
            };

            // Grown by the standoff, then expressed relative to this frame's
            // own origin — the space the text is laid out in.
            let x = bounds.x - standoff.left - mine.x;
            let y = bounds.y - standoff.top - mine.y;
            let width = bounds.width + standoff.left + standoff.right;
            let height = bounds.height + standoff.top + standoff.bottom;

            // Nowhere near this frame's measure: skip it rather than hand the
            // breaker a rectangle it will only ignore.
            if x > measure || x + width < 0.0 || y + height < 0.0 {
                continue;
            }
            // The shape, when the wrap asks for it: the outline in document
            // space, flattened to a polyline and moved into the text's space
            // the same way the box was.
            let shape = match wrap {
                tessera_document::nodes::TextWrap::Contour { standoff, .. } => doc
                    .outline(other)
                    .map(|path| {
                        let mut outline: Vec<(f64, f64)> = Vec::new();
                        kurbo::flatten(path.iter(), 0.25, |el| match el {
                            kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => {
                                outline.push((p.x - mine.x, p.y - mine.y));
                            }
                            _ => {}
                        });
                        tessera_text::wrap::Blocking::Contour { outline, standoff }
                    })
                    .unwrap_or_default(),
                tessera_document::nodes::TextWrap::Jump => tessera_text::wrap::Blocking::Jump,
                _ => tessera_text::wrap::Blocking::Bounds,
            };
            out.push(tessera_text::wrap::Obstacle {
                x,
                y,
                width,
                height,
                shape,
                sides: wrap.sides(),
            });
        }
    }
    let _ = frame;
    out
}

/// How far into its story a threaded frame begins.
///
/// Walks the chain from its first frame, laying each one out to find where it
/// stopped. There is no cheaper answer: how much a frame holds depends on its
/// own measure and its own columns, so the frames before it really do have to
/// be laid out to know where this one starts.
///
/// A frame that is not threaded returns zero without laying anything out,
/// which is every frame in most documents.
fn story_starts_at<'a>(
    doc: &'a Document,
    shaper: &mut Shaper,
    frame: FrameId,
    composed: Option<(StoryId, &'a TextStory)>,
    running: &Running,
) -> usize {
    let chain = doc.thread_of(frame);
    let Some(at) = chain.iter().position(|f| *f == frame) else {
        return 0;
    };
    if at == 0 {
        return 0;
    }

    let mut from = 0usize;
    for id in &chain[..at] {
        let Some(before) = doc.frame(*id) else {
            continue;
        };
        let FrameKind::Text { story, .. } = &before.kind else {
            continue;
        };
        let Some(text) = story_of(doc, composed, *story) else {
            continue;
        };

        // Each earlier frame is composed on its own page: a "continued on
        // page 9" in frame one is one character wide on page 8 and could be
        // two on page 98, and the break it moves decides where this frame's
        // text starts.
        let Some(on) = doc.page_of_frame(*id) else {
            continue;
        };
        let flowed = compose_frame(
            doc, shaper, *id, before, *story, text, from, on, running, composed,
        );
        // A frame that held nothing hands the story on untouched rather than
        // restarting it: treating "placed nothing" as zero would loop the
        // whole chain back to the beginning.
        if let Some(to) = flowed.consumed_to {
            from = to;
        }
    }
    from
}

#[allow(clippy::too_many_arguments)]
fn compose_frame(
    doc: &Document,
    shaper: &mut Shaper,
    id: FrameId,
    frame: &tessera_document::nodes::Frame,
    story_id: StoryId,
    story: &TextStory,
    from: usize,
    on: PageId,
    running: &Running,
    composed: Option<(StoryId, &TextStory)>,
) -> tessera_text::shape::Flowed {
    let FrameKind::Text { layout, .. } = &frame.kind else {
        unreachable!()
    };
    // Shaped at the width of a column, then flowed through them.
    // One shaping serves every column because they are all the same
    // width, which is what makes columns a cheap pass over a finished
    // layout rather than a shaping each.
    let columns = layout.columns_of(DocRect {
        x: 0.0,
        y: 0.0,
        width: frame.bounds.width,
        height: frame.bounds.height,
    });
    let measure = columns.first().map_or(frame.bounds.width, |c| c.width);
    let boxes: Vec<tessera_text::shape::Column> = columns
        .iter()
        .map(|c| tessera_text::shape::Column {
            x: c.x,
            y: c.y,
            width: c.width,
            height: c.height,
        })
        .collect();

    // The document's enum mapped onto the shaper's. Two enums rather
    // than one because `tessera_text` knows nothing about documents,
    // the same arrangement `Styles` uses.
    let vertical = match layout.vertical {
        tessera_document::nodes::VerticalJustify::Top => tessera_text::shape::Vertical::Top,
        tessera_document::nodes::VerticalJustify::Centre => tessera_text::shape::Vertical::Centre,
        tessera_document::nodes::VerticalJustify::Bottom => tessera_text::shape::Vertical::Bottom,
        tessera_document::nodes::VerticalJustify::Justify => tessera_text::shape::Vertical::Justify,
    };

    // Where in the story this frame starts. Zero unless something
    // flows into it, in which case the frames before it are laid out
    // to find out how much they hold — the answer depends on their
    // measures, so there is no shortcut past doing it.

    // The grid is measured from the top of the **page**, so two
    // frames on the same page line up. The text crate has no notion of
    // a page, so the page's rhythm is expressed in this frame's own
    // space before it is handed over: a slot at document `y` is at
    // `y - the frame's top` inside the frame.
    //
    // A rotated frame is left off the grid. A rhythm measured down the
    // page means nothing to text running across it at an angle, and
    // guessing would be worse than declining.
    let grid = doc.setup.baseline_grid.and_then(|grid| {
        if !layout.lock_to_grid || grid.step <= 0.0 || !frame.transform.is_identity() {
            return None;
        }
        let page = doc.pages.get(doc.page_of_frame(id)?)?.bounds;
        Some(tessera_text::shape::Grid {
            first: page.y + grid.start - frame.bounds.y,
            step: grid.step,
        })
    });

    // Objects on the same spread that this text must run around,
    // in the text's own space. Gathered per frame rather than once,
    // because "near" is relative to the frame doing the reading.
    let obstacles = obstacles_for(doc, id, frame, measure);

    // Room for anything anchored in this story. The boxes come from
    // the anchored frames' own sizes, so a picture made larger pushes
    // the copy aside the moment it is resized.
    let anchored: Vec<tessera_text::shape::InlineObject> = doc
        .inline_objects_of(story_id)
        .into_iter()
        .map(|(at, _, width, height)| tessera_text::shape::InlineObject { at, width, height })
        .collect();
    // What each footnote is numbered, by the document's options: the
    // count restarts where the options say, and is written as they say.
    let labels = footnote_labels(doc, shaper, id, story, composed, running, on);

    // The document resolves the styles; the page says what the markers read
    // as. One object answering both, so the shaper asks one question.
    let mut variables = variables_for(doc, id, on, running);
    variables.footnote_labels = labels.clone();
    let styles = OnPage::new(doc, variables);
    let shaped =
        shaper.shape_around_with_objects(story, &styles, measure, from, &obstacles, &anchored);

    // The footnotes, shaped at the column's measure and numbered as the
    // references are, for the flow to set at the foot of whichever column
    // their references land in. Shaped here because the flow has no shaper,
    // and all of them rather than the ones after `from`: the flow keeps only
    // those whose line it places. None when the notes are endnotes: they
    // are gathered into a story of their own, and the foot stays copy.
    let at_end = doc.footnotes.placement == tessera_document::footnotes::NotePlacement::End;
    let notes: Vec<tessera_text::shape::Note> = story
        .footnote_offsets()
        .into_iter()
        .zip(&story.footnotes)
        .enumerate()
        .filter(|_| !at_end)
        .map(|(n, (at, note))| {
            let label = labels
                .get(n)
                .cloned()
                .unwrap_or_else(|| (n + 1).to_string());
            let numbered = OnPage::new(doc, Variables::for_footnote_labelled(n as u32 + 1, label));
            tessera_text::shape::Note {
                at,
                text: shaper.shape(note, &numbered, measure),
            }
        })
        .collect();
    let options = doc.footnotes;
    let note_layout = tessera_text::shape::NoteLayout {
        space_before: options.space_before,
        space_between: options.space_between,
        rule: options
            .rule
            .then_some((options.rule_weight, options.rule_fraction)),
    };
    tessera_text::shape::flow_with_notes(shaped, &boxes, vertical, grid, &notes, &note_layout)
}

/// The label of every footnote in `story`, for the frame `id` on page `on`.
///
/// Counting from the options' start, in their numbering; restarting per
/// page means counting from the first reference in the first frame of the
/// thread that stands on this page — the frames before it are composed to
/// find where that frame begins, which is the same work `story_starts_at`
/// does and costs the same.
fn footnote_labels(
    doc: &Document,
    shaper: &mut Shaper,
    id: FrameId,
    story: &TextStory,
    composed: Option<(StoryId, &TextStory)>,
    running: &Running,
    on: PageId,
) -> Vec<String> {
    let options = doc.footnotes;
    let count = story.footnotes.len();
    if count == 0 {
        return Vec::new();
    }
    // Endnotes count once through the story: a list at the end has no
    // pages to restart on, and its references must read as it does.
    let restart = if options.placement == tessera_document::footnotes::NotePlacement::End {
        tessera_document::footnotes::Restart::Never
    } else {
        options.restart
    };
    let base = match restart {
        tessera_document::footnotes::Restart::Never => 0,
        tessera_document::footnotes::Restart::Page => {
            // The first frame of the chain on this page, and where it starts.
            let chain = doc.thread_of(id);
            let first_here = chain
                .iter()
                .copied()
                .find(|f| doc.page_of_frame(*f) == Some(on))
                .unwrap_or(id);
            let from = if first_here == id {
                story_starts_at(doc, shaper, id, composed, running)
            } else {
                story_starts_at(doc, shaper, first_here, composed, running)
            };
            story.footnote_index_at(from)
        }
    };
    (0..count)
        .map(|n| {
            let number = (n as i64 - base as i64 + i64::from(options.start_at)).max(1) as u32;
            options.numbering.label(number)
        })
        .collect()
}

/// The hyperlinks in `story` as laid out in `shaped`: each linked run's
/// rectangles, from the same geometry a selection is drawn with, so a link
/// covers exactly what looks linked.
fn links_in(
    doc: &Document,
    story: &TextStory,
    shaped: &ShapedText,
    width: f32,
) -> Vec<ResolvedLink> {
    use tessera_text::edit::TextCursor;
    use tessera_text::story::Hyperlink;

    let pages: Vec<PageId> = doc.page_ids().collect();
    let mut out = Vec::new();
    for run in &story.runs {
        let target = match story.resolve_run(run, doc).link {
            Some(Hyperlink::Url(url)) if !url.trim().is_empty() => LinkTarget::Url(url),
            Some(Hyperlink::Destination(name)) => {
                let Some(page) = doc.destination_page(&name) else {
                    continue; // a link to nowhere is no link
                };
                let Some(index) = pages.iter().position(|p| *p == page) else {
                    continue;
                };
                LinkTarget::Page(index)
            }
            _ => continue,
        };
        let geometry = shaped.caret_geometry(
            TextCursor {
                position: run.range.end,
                anchor: run.range.start,
            },
            width,
        );
        let rects: Vec<DocRect> = geometry
            .selection
            .iter()
            .filter(|r| r.width() > 0.0 && r.height() > 0.0)
            .map(|r| DocRect {
                x: r.x0,
                y: r.y0,
                width: r.width(),
                height: r.height(),
            })
            .collect();
        if rects.is_empty() {
            continue;
        }
        out.push(ResolvedLink { rects, target });
    }
    out
}

/// What the markers in `frame`'s text read as when it stands on `on`.
///
/// A page not in the reading order — a parent's — has no number, and its
/// marker reads as the parent's prefix, which is what every layout tool shows
/// on a parent page and what tells a person the marker is there.
fn variables_for(doc: &Document, frame: FrameId, on: PageId, running: &Running) -> Variables {
    let label_of = |page: PageId| -> String {
        doc.page_label(page).unwrap_or_else(|| {
            doc.master_ids()
                .find(|m| doc.pages_of_master(*m).contains(&page))
                .and_then(|m| doc.masters.get(m))
                .map(|m| m.name.split('-').next().unwrap_or("").trim().to_owned())
                .unwrap_or_default()
        })
    };
    let number = doc.page_number(on);

    // The pages holding the frames either side of this one in its thread —
    // "continued on page 9", "continued from page 7".
    let chain = doc.thread_of(frame);
    let at = chain.iter().position(|f| *f == frame);
    let neighbour = |offset: isize| -> String {
        at.and_then(|i| i.checked_add_signed(offset))
            .and_then(|i| chain.get(i))
            .and_then(|f| doc.page_of_frame(*f))
            .map(label_of)
            .unwrap_or_default()
    };

    Variables {
        page_number: label_of(on),
        next_page_number: neighbour(1),
        previous_page_number: neighbour(-1),
        section_marker: number.map(|n| n.marker).unwrap_or_default(),
        variables: doc
            .variables
            .iter()
            .map(|v| match &v.kind {
                tessera_document::variables::VariableKind::Custom(text) => text.clone(),
                tessera_document::variables::VariableKind::RunningHeader { style, which } => {
                    running.header(on, *style, *which).unwrap_or_default()
                }
            })
            .collect(),
        footnote_number: None,
        footnote_text: None,
        footnote_labels: Vec::new(),
        cross_references: cross_references_for(doc, frame, running, &label_of),
    }
}

/// What each cross-reference in `frame`'s story reads as: the page its
/// target is on, the paragraph its anchor stands in, or both. An anchor
/// nobody has laid out yet, or a name nothing has, reads as a question
/// mark — the shaper's placeholder — so a reader can see there is something
/// to fix rather than nothing at all.
fn cross_references_for(
    doc: &Document,
    frame: FrameId,
    running: &Running,
    label_of: &dyn Fn(PageId) -> String,
) -> Vec<String> {
    use tessera_text::story::CrossReferenceFormat;
    let Some(FrameKind::Text { story, .. }) = doc.frame(frame).map(|f| &f.kind) else {
        return Vec::new();
    };
    let Some(story) = doc.story(*story) else {
        return Vec::new();
    };
    story
        .cross_references
        .iter()
        .map(|reference| {
            let (page, paragraph) = match running.anchor(&reference.target) {
                Some((page, paragraph)) => (Some(*page), Some(paragraph.as_str())),
                // A named page destination — the contents' targets — has a
                // page and no paragraph.
                None => (doc.destination_page(&reference.target), None),
            };
            let page = page.map(label_of);
            match (reference.format, page, paragraph) {
                (CrossReferenceFormat::PageNumber, Some(page), _) => page,
                (CrossReferenceFormat::ParagraphText, _, Some(text)) if !text.is_empty() => {
                    text.to_owned()
                }
                (CrossReferenceFormat::ParagraphAndPage, Some(page), Some(text))
                    if !text.is_empty() =>
                {
                    format!("{text} on page {page}")
                }
                (CrossReferenceFormat::ParagraphAndPage, Some(page), _) => format!("page {page}"),
                _ => "?".to_owned(),
            }
        })
        .collect()
}

/// One frame, resolved.
///
/// Pulled out of the walk so that a master's item can be resolved the same way
/// a page's own is — the difference between them is where it lands, not what
/// it is. `None` for a frame that draws nothing: a group, or a text frame
/// whose story has gone.
fn resolve_one<'a>(
    doc: &'a Document,
    shaper: &mut Shaper,
    id: FrameId,
    frame: &tessera_document::nodes::Frame,
    composed: Option<(StoryId, &'a TextStory)>,
    on: PageId,
    running: &Running,
) -> Option<ResolvedItem> {
    let mut links = Vec::new();
    let kind = match &frame.kind {
        FrameKind::Rectangle => ResolvedKind::Rectangle {
            outline: frame.corners.outline(frame.bounds),
            fill: doc.resolve_paint(&frame.fill),
            stroke: resolved_stroke(doc, frame.stroke.as_ref()),
        },
        FrameKind::Ellipse => ResolvedKind::Ellipse {
            fill: doc.resolve_paint(&frame.fill),
            stroke: resolved_stroke(doc, frame.stroke.as_ref()),
        },
        FrameKind::Path(path) => {
            let path = fit_to_bounds(path, frame.bounds);
            // Type on a path: the story shaped to the length it may use and
            // walked along the curve. Drawn in the story's colour, as a text
            // frame's is.
            let text = doc.path_text(id).and_then(|carried| {
                let story = story_of(doc, composed, carried.story)?;
                let placement = crate::path_text::Placement {
                    start: carried.start,
                    end: carried.end,
                    align: match carried.align {
                        tessera_document::path_text::PathTextAlign::Baseline => {
                            crate::path_text::Align::Baseline
                        }
                        tessera_document::path_text::PathTextAlign::Centre => {
                            crate::path_text::Align::Centre
                        }
                        tessera_document::path_text::PathTextAlign::Ascender => {
                            crate::path_text::Align::Ascender
                        }
                        tessera_document::path_text::PathTextAlign::Descender => {
                            crate::path_text::Align::Descender
                        }
                    },
                    flip: carried.flip,
                };
                let measure = crate::path_text::measure(&path, &placement);
                let shaped = shaper.shape(story, doc, measure);
                let colour = story
                    .runs
                    .first()
                    .map(|run| story.resolve_run(run, doc))
                    .and_then(|f| f.colour)
                    .unwrap_or(tessera_color::Color::BLACK);
                Some((
                    crate::path_text::place(&shaped, &path, &placement),
                    doc.resolve_colour(&colour),
                ))
            });
            ResolvedKind::Path {
                path,
                // An open path with no explicit stroke would be invisible, so
                // a path frame's fill is treated as its stroke colour when it
                // has no stroke of its own.
                fill: None,
                stroke: Some(
                    resolved_stroke(doc, frame.stroke.as_ref()).unwrap_or_else(|| {
                        // A stroke is a single colour, so a gradient-filled path
                        // standing in for its own stroke takes one colour from the
                        // ramp rather than pretending to draw the ramp along it.
                        // Gradient strokes are not modelled.
                        Stroke::new(doc.resolve_colour(&frame.fill.representative()), 1.0)
                    }),
                ),
                text,
            }
        }

        // A group draws nothing of its own, and paint_order already
        // expanded it into its children, so it never reaches here.
        FrameKind::Group(_) => return None,

        FrameKind::Graphic { placed } => {
            let link = placed.and_then(|p| doc.links.get(p.link).cloned());
            let missing = match (&placed, &link) {
                // Nothing placed: an empty frame, which is a real thing rather
                // than a fault — it is the box somebody drew to reserve room.
                (None, _) => false,
                (Some(_), Some(l)) => l.status() == tessera_document::links::Status::Missing,
                // Placed, but the link has gone from the table. A broken
                // document rather than a broken file.
                (Some(_), None) => true,
            };
            ResolvedKind::Graphic {
                inner: placed.map(|p| p.inner).unwrap_or(Transform::IDENTITY),
                source: link.as_ref().filter(|_| !missing).map(|l| l.path.clone()),
                natural: link.map(|l| l.natural).unwrap_or((0.0, 0.0)),
                missing,
                stroke: resolved_stroke(doc, frame.stroke.as_ref()),
            }
        }

        FrameKind::Table(table) => {
            // Cells hold stories like any text frame, so a composing input
            // method reaches them the same way.
            let laid = crate::table::lay_out(table, doc, shaper, |id| {
                story_of(doc, composed, id).cloned()
            });
            ResolvedKind::Table {
                laid,
                stroke: resolved_stroke(doc, table.stroke.as_ref()),
            }
        }

        FrameKind::Text {
            story: story_id,
            layout: _,
        } => {
            // A text frame whose story is missing is a broken document,
            // not a blank frame. Skipping it silently would hide the
            // breakage; milestone 6's preflight reports it. For now it
            // simply does not paint, which is visible.
            let story = story_of(doc, composed, *story_id)?;
            // The document is what resolves named styles, so it is what
            // the shaper is handed.
            //
            // Colour is still one per frame rather than one per run: the
            // shaper's brush is `()` and the consumer paints, so a run's
            // own colour has nowhere to travel yet. Taken from the first
            // run's resolved format, which is right for every story that
            // has one colour and wrong for none that exist today.
            let colour = story
                .runs
                .first()
                .map(|run| story.resolve_run(run, doc))
                .and_then(|f| f.colour)
                .unwrap_or(tessera_color::Color::BLACK);
            let colour = doc.resolve_colour(&colour);
            let from = story_starts_at(doc, shaper, id, composed, running);
            let flowed = compose_frame(
                doc, shaper, id, frame, *story_id, story, from, on, running, composed,
            );

            links = links_in(doc, story, &flowed.text, frame.bounds.width as f32);
            ResolvedKind::Text {
                shaped: flowed.text,
                color: colour,
                overset_lines: flowed.overset_lines,
            }
        }
    };

    Some(ResolvedItem {
        frame: id,
        links,
        on: Some(on),
        bounds: frame.bounds,
        transform: frame.transform,
        spread_area: doc.spread_of_frame(id).and_then(|s| doc.spread_area(s)),
        blend: frame.blend,
        // The shadow's colour goes through the swatch table like every other,
        // so a shadow tinted with a named colour follows it.
        shadow: frame
            .shadow
            .as_ref()
            .map(|s| tessera_document::shadow::Shadow {
                colour: doc.resolve_colour(&s.colour),
                ..s.clone()
            }),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::ids::MasterId;
    use tessera_document::nodes::Frame;
    use tessera_text::story::Story;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Frame {
        Frame {
            bounds: DocRect {
                x,
                y,
                width: w,
                height: h,
            },
            kind: FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            anchor: None,
            style: None,
        }
    }

    // --- anchored objects ----------------------------------------------------

    /// A text frame whose story carries one marker, and a frame anchored to it.
    ///
    /// Returns the document, the host, and the anchored frame.
    fn a_text_frame_with_an_anchored_picture(
        before: &str,
        after: &str,
        size: (f64, f64),
    ) -> (Document, FrameId, FrameId) {
        use tessera_document::anchored::{Anchored, MARKER};

        let mut doc = Document::default();
        let page = doc.page_ids().next().expect("a page");
        let layer = doc.default_layer().expect("a layer");
        let bounds = doc.pages[page].bounds;

        let story = doc.add_story(Story::new(format!("{before}{MARKER}{after}")));
        let host = doc.add_frame(layer, {
            let mut f = rect(bounds.x + 20.0, bounds.y + 20.0, 300.0, 300.0);
            f.kind = FrameKind::text(story);
            f
        });

        let mut picture = rect(0.0, 0.0, size.0, size.1);
        picture.anchor = Some(Anchored::new(story, 0));
        let anchored = doc.add_frame(layer, picture);

        (doc, host, anchored)
    }

    fn item_for(resolved: &ResolvedDocument, id: FrameId) -> Option<&ResolvedItem> {
        resolved.items.iter().find(|i| i.frame == id)
    }

    #[test]
    fn an_anchored_frame_lands_where_its_marker_is() {
        let (doc, host, anchored) =
            a_text_frame_with_an_anchored_picture("Some words ", " and more", (40.0, 20.0));
        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        let host_item = item_for(&resolved, host).expect("the host resolved");
        let item = item_for(&resolved, anchored).expect("the anchored frame resolved");

        // Its own bounds are at the origin; the transform is what places it.
        let placed = item.transform.apply(tessera_geometry::DocPoint {
            x: item.bounds.x,
            y: item.bounds.y,
        });
        assert!(
            placed.x > host_item.bounds.x,
            "it must sit after the words before it, not at the frame's edge"
        );
        assert!(
            placed.x < host_item.bounds.x + 300.0 && placed.y >= host_item.bounds.y,
            "and inside the frame that hosts it: {placed:?}"
        );
    }

    #[test]
    fn an_anchored_frame_is_drawn_once_not_twice() {
        // It is skipped by the ordinary walk and placed by its host. Resolved
        // in both, it would paint at its stale position as well as its real
        // one — two pictures where the document has one.
        let (doc, _host, anchored) =
            a_text_frame_with_an_anchored_picture("x ", " y", (30.0, 30.0));
        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        let count = resolved
            .items
            .iter()
            .filter(|i| i.frame == anchored)
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn the_text_makes_room_for_what_is_anchored_in_it() {
        // The point of anchoring rather than merely positioning: the copy has
        // to give way, or the picture is printed on top of the words.
        // Text that already fills most of the measure, and an object nearly as
        // wide as the whole of it: the copy has nowhere to go but down.
        let (doc, host, _) = a_text_frame_with_an_anchored_picture(
            "some words that already run most of the way across ",
            " and a good deal more after it",
            (260.0, 12.0),
        );
        let plain = {
            let mut d = doc.clone();
            // The same story with the marker's frame unanchored, so nothing is
            // reserved for it.
            let ids: Vec<FrameId> = d.frames.keys().collect();
            for id in ids {
                if let Some(f) = d.frames.get_mut(id) {
                    f.anchor = None;
                }
            }
            d
        };

        let mut shaper = Shaper::new();
        let with = resolve(&doc, &mut shaper);
        let without = resolve(&plain, &mut shaper);

        let lines = |r: &ResolvedDocument| {
            item_for(r, host)
                .and_then(|i| match &i.kind {
                    ResolvedKind::Text { shaped, .. } => Some(shaped.lines.len()),
                    _ => None,
                })
                .unwrap_or(0)
        };
        assert!(
            lines(&with) > lines(&without),
            "a 150pt object in a 300pt measure must push the copy onto more lines: {} vs {}",
            lines(&with),
            lines(&without)
        );
    }

    #[test]
    fn an_anchored_frame_moves_when_the_copy_before_it_grows() {
        // The whole reason to anchor rather than place: add a sentence above
        // and the picture goes down the page with its paragraph.
        let short = a_text_frame_with_an_anchored_picture("one line ", "", (20.0, 10.0));
        let long = a_text_frame_with_an_anchored_picture(
            "a much longer run of copy that will certainly take several lines \
             before it ever reaches the marker at the end of it ",
            "",
            (20.0, 10.0),
        );

        let mut shaper = Shaper::new();
        let y_of = |(doc, _host, anchored): &(Document, FrameId, FrameId), s: &mut Shaper| {
            let resolved = resolve(doc, s);
            let item = item_for(&resolved, *anchored).expect("resolved");
            item.transform
                .apply(tessera_geometry::DocPoint {
                    x: item.bounds.x,
                    y: item.bounds.y,
                })
                .y
        };

        let near = y_of(&short, &mut shaper);
        let far = y_of(&long, &mut shaper);
        assert!(
            far > near,
            "more copy above it must push it down the page: {far} vs {near}"
        );
    }

    #[test]
    fn a_story_with_no_markers_costs_nothing() {
        let mut doc = Document::default();
        let page = doc.page_ids().next().expect("a page");
        let layer = doc.default_layer().expect("a layer");
        let bounds = doc.pages[page].bounds;
        let story = doc.add_story(Story::new("no anchors here at all"));
        let host = doc.add_frame(layer, {
            let mut f = rect(bounds.x, bounds.y, 200.0, 100.0);
            f.kind = FrameKind::text(story);
            f
        });

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);
        assert_eq!(resolved.items.len(), 1);
        assert!(item_for(&resolved, host).is_some());
    }

    #[test]
    fn an_empty_document_resolves_to_nothing() {
        let resolved = resolve(&Document::new(), &mut Shaper::new());
        assert!(resolved.items.is_empty());
    }

    #[test]
    fn a_rectangle_resolves_to_a_rectangle_at_its_bounds() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(layer, rect(5.0, 6.0, 20.0, 30.0));

        let resolved = resolve(&doc, &mut Shaper::new());

        assert_eq!(resolved.items.len(), 1);
        assert_eq!(resolved.items[0].bounds.width, 20.0);
        assert!(matches!(
            resolved.items[0].kind,
            ResolvedKind::Rectangle { .. }
        ));
    }

    #[test]
    fn items_come_out_in_paint_order() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let first = doc.add_frame(layer, rect(0.0, 0.0, 1.0, 1.0));
        let second = doc.add_frame(layer, rect(0.0, 0.0, 2.0, 2.0));

        let resolved = resolve(&doc, &mut Shaper::new());

        assert_eq!(resolved.items[0].frame, first);
        assert_eq!(resolved.items[1].frame, second);
    }

    #[test]
    fn a_text_frame_resolves_with_text_shaped_to_the_frame_width() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(Story::new("Hello"));

        let mut frame = rect(0.0, 0.0, 500.0, 100.0);
        frame.kind = FrameKind::text(story);
        doc.add_frame(layer, frame);

        let resolved = resolve(&doc, &mut Shaper::new());

        let ResolvedKind::Text { shaped, .. } = &resolved.items[0].kind else {
            panic!("expected text");
        };
        assert_eq!(shaped.glyph_count(), 5);
    }

    #[test]
    fn a_narrow_text_frame_wraps_because_the_frame_width_is_the_measure() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(Story::new("the quick brown fox jumps over"));

        let mut frame = rect(0.0, 0.0, 60.0, 100.0);
        frame.kind = FrameKind::text(story);
        doc.add_frame(layer, frame);

        let resolved = resolve(&doc, &mut Shaper::new());

        let ResolvedKind::Text { shaped, .. } = &resolved.items[0].kind else {
            panic!("expected text");
        };
        assert!(
            shaped.lines.len() > 1,
            "the frame width must bound the text"
        );
    }

    #[test]
    fn a_hidden_layer_contributes_nothing() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(layer, rect(0.0, 0.0, 10.0, 10.0));
        doc.layers.get_mut(layer).expect("layer").visible = false;

        assert_eq!(resolve(&doc, &mut Shaper::new()).items.len(), 0);
    }

    #[test]
    fn a_text_frame_with_a_missing_story_does_not_paint() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(Story::new("gone"));
        doc.stories.remove(story);

        let mut frame = rect(0.0, 0.0, 100.0, 20.0);
        frame.kind = FrameKind::text(story);
        doc.add_frame(layer, frame);

        assert!(resolve(&doc, &mut Shaper::new()).items.is_empty());
    }

    fn path_frame(bounds: DocRect, path: kurbo::BezPath) -> Frame {
        Frame {
            bounds,
            kind: FrameKind::Path(path),
            transform: Transform::IDENTITY,
            fill: Paint::Solid(Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            anchor: None,
            style: None,
        }
    }

    /// A diagonal line filling a 10x10 box.
    fn diagonal() -> kurbo::BezPath {
        let mut p = kurbo::BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 10.0));
        p
    }

    #[test]
    fn a_path_with_text_resolves_its_glyphs_along_the_curve() {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(Story::new("Set along the line"));
        let bounds = DocRect {
            x: 20.0,
            y: 20.0,
            width: 300.0,
            height: 10.0,
        };
        let mut line = kurbo::BezPath::new();
        line.move_to((0.0, 5.0));
        line.line_to((300.0, 5.0));
        let id = doc.add_frame(layer, path_frame(bounds, line));

        // A bare path carries nothing.
        let resolved = resolve(&doc, &mut Shaper::new());
        let ResolvedKind::Path { text, .. } = &resolved.items[0].kind else {
            panic!("a path");
        };
        assert!(text.is_none());

        doc.set_path_text(id, Some(tessera_document::path_text::PathText::new(story)));
        let resolved = resolve(&doc, &mut Shaper::new());
        let ResolvedKind::Path {
            path,
            text: Some((placed, colour)),
            ..
        } = &resolved.items[0].kind
        else {
            panic!("a path with text");
        };
        assert_eq!(*colour, Color::BLACK);
        assert_eq!(placed.overset_lines, 0);
        let glyphs: Vec<_> = placed.runs.iter().flat_map(|r| &r.glyphs).collect();
        assert_eq!(glyphs.len(), "Set along the line".len());
        // In the same coordinates as the path the renderers draw — the one
        // fitted to the frame — on its line, upright.
        let kurbo::PathEl::MoveTo(start) = path.elements()[0] else {
            panic!("a move");
        };
        for g in &glyphs {
            assert!((g.y - start.y).abs() < 1e-6, "{} vs {}", g.y, start.y);
            assert!(g.angle.abs() < 1e-9);
        }
        assert!(!placed.fonts.is_empty(), "the font travels with the glyphs");
    }

    #[test]
    fn a_path_is_scaled_to_fill_its_frame() {
        use kurbo::Shape as _;
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        // Same path, but a frame twice as wide and three times as tall.
        doc.add_frame(
            layer,
            path_frame(
                DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 30.0,
                },
                diagonal(),
            ),
        );

        let resolved = resolve(&doc, &mut Shaper::new());
        let ResolvedKind::Path { path, .. } = &resolved.items[0].kind else {
            panic!("expected a path");
        };
        let b = path.bounding_box();

        assert!((b.width() - 20.0).abs() < 1e-9, "width was {}", b.width());
        assert!(
            (b.height() - 30.0).abs() < 1e-9,
            "height was {}",
            b.height()
        );
    }

    #[test]
    fn a_path_that_already_fits_is_left_alone() {
        use kurbo::Shape as _;
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(
            layer,
            path_frame(
                DocRect {
                    x: 5.0,
                    y: 5.0,
                    width: 10.0,
                    height: 10.0,
                },
                diagonal(),
            ),
        );

        let resolved = resolve(&doc, &mut Shaper::new());
        let ResolvedKind::Path { path, .. } = &resolved.items[0].kind else {
            panic!("expected a path");
        };

        assert_eq!(path.bounding_box(), diagonal().bounding_box());
    }

    #[test]
    fn a_horizontal_line_scales_across_but_is_not_flattened_further() {
        use kurbo::Shape as _;
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let mut flat = kurbo::BezPath::new();
        flat.move_to((0.0, 0.0));
        flat.line_to((10.0, 0.0));

        doc.add_frame(
            layer,
            path_frame(
                DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 50.0,
                    height: 0.0,
                },
                flat,
            ),
        );

        let resolved = resolve(&doc, &mut Shaper::new());
        let ResolvedKind::Path { path, .. } = &resolved.items[0].kind else {
            panic!("expected a path");
        };
        let b = path.bounding_box();

        assert!((b.width() - 50.0).abs() < 1e-9, "width was {}", b.width());
        assert!(
            b.height().abs() < 1e-9,
            "an axis with no extent must not blow up"
        );
    }

    // --- parent pages -------------------------------------------------------

    #[test]
    fn a_master_item_is_drawn_on_the_page_that_inherits_it() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let bounds = doc.pages[on].bounds;
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(layer, rect(bounds.x + 10.0, bounds.y + 10.0, 20.0, 20.0));

        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        assert_eq!(
            resolved.items.len(),
            1,
            "once, on the page — the parent is not on this canvas"
        );
        let item = &resolved.items[0];
        let landed = item.transform.apply(item.bounds.center());
        let target = doc.pages[page].bounds;
        assert!(
            landed.x >= target.x
                && landed.x <= target.x + target.width
                && landed.y >= target.y
                && landed.y <= target.y + target.height,
            "and it landed on that page, not on the master: {landed:?}"
        );
    }

    #[test]
    fn a_parent_is_not_on_the_documents_canvas() {
        // A parent is edited in isolation. Sitting it beside the document put
        // a second set of pages into the scroll a person is laying out in, and
        // made it a permanent fixture nobody asked for.
        let mut doc = Document::new();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let bounds = doc.pages[on].bounds;
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(layer, rect(bounds.x + 10.0, bounds.y + 10.0, 20.0, 20.0));

        let mut shaper = Shaper::new();
        assert!(resolve(&doc, &mut shaper).items.is_empty());
    }

    #[test]
    fn the_documents_canvas_holds_only_the_documents_sheets() {
        let mut doc = Document::new();
        let before = resolve(&doc, &mut Shaper::new()).pages.len();
        doc.add_master("A-Master");

        let after = resolve(&doc, &mut Shaper::new()).pages.len();
        assert_eq!(
            after, before,
            "adding a parent adds no sheet to the document"
        );
    }

    #[test]
    fn opening_a_parent_shows_the_parent_and_nothing_else() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let bounds = doc.pages[on].bounds;
        let layer = doc.default_layer().expect("layer");
        let furniture = doc.add_frame(layer, rect(bounds.x + 10.0, bounds.y + 10.0, 20.0, 20.0));

        // Something on the document, which must not appear.
        let page = doc.pages[doc.page_ids().next().expect("a page")].bounds;
        doc.add_frame(layer, rect(page.x + 5.0, page.y + 5.0, 10.0, 10.0));

        let mut shaper = Shaper::new();
        let resolved = resolve_scope(&doc, &mut shaper, Scope::Master(master));

        assert_eq!(resolved.items.len(), 1, "the parent alone");
        assert_eq!(resolved.items[0].frame, furniture);
        assert_eq!(resolved.pages.len(), 1, "and its one sheet");
    }

    #[test]
    fn a_parent_being_edited_shows_no_page_it_is_applied_to() {
        // The isolation runs both ways: opening a parent must not drag in the
        // pages built on it, or "edit the parent" would mean "edit everything".
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let bounds = doc.pages[on].bounds;
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(layer, rect(bounds.x + 10.0, bounds.y + 10.0, 20.0, 20.0));
        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));

        let mut shaper = Shaper::new();
        let resolved = resolve_scope(&doc, &mut shaper, Scope::Master(master));

        assert_eq!(resolved.items.len(), 1, "drawn once, on the parent");
    }

    #[test]
    fn a_master_item_is_drawn_once_for_each_page_that_inherits_it() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let bounds = doc.pages[on].bounds;
        let layer = doc.default_layer().expect("layer");
        doc.add_frame(layer, rect(bounds.x + 10.0, bounds.y + 10.0, 20.0, 20.0));
        doc.add_page();
        doc.add_page();
        for page in doc.page_ids().collect::<Vec<_>>() {
            doc.apply_master(page, Some(master));
        }

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        assert_eq!(resolved.items.len(), 3, "one drawing, three pages");
        // Each on its own spread, which is what keeps it clipped to its sheet.
        let areas: Vec<_> = resolved.items.iter().map(|i| i.spread_area).collect();
        assert!(
            areas.iter().all(|a| a.is_some()),
            "every inherited item knows its sheet"
        );
        assert_ne!(areas[0], areas[1], "and they are different sheets");
    }

    // --- page numbers, sections, running headers -----------------------------

    /// A document of `pages` pages, none facing, every one built on a master
    /// that carries one text frame saying `text`.
    fn a_master_folio(pages: usize, text: &str) -> (Document, FrameId, MasterId) {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let bounds = doc.pages[on].bounds;
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(Story::new(text));
        let folio = doc.add_frame(layer, {
            let mut f = rect(bounds.x + 10.0, bounds.y + 10.0, 300.0, 40.0);
            f.kind = FrameKind::text(story);
            f
        });
        for _ in 1..pages {
            doc.add_page();
        }
        for page in doc.page_ids().collect::<Vec<_>>() {
            doc.apply_master(page, Some(master));
        }
        (doc, folio, master)
    }

    fn glyphs_of(item: &ResolvedItem) -> Vec<u32> {
        let ResolvedKind::Text { shaped, .. } = &item.kind else {
            panic!("not text");
        };
        shaped
            .lines
            .iter()
            .flat_map(|l| l.glyphs().map(|g| g.glyph_id))
            .collect()
    }

    /// The glyphs `text` shapes to, for comparing against what a marker
    /// became: the resolved item holds glyphs, not characters.
    fn glyphs_for(shaper: &mut Shaper, text: &str) -> Vec<u32> {
        let shaped = shaper.shape(
            &Story::new(text),
            &tessera_text::story::NoStyles::default(),
            300.0,
        );
        shaped
            .lines
            .iter()
            .flat_map(|l| l.glyphs().map(|g| g.glyph_id))
            .collect()
    }

    #[test]
    fn a_master_folio_reads_each_pages_own_number() {
        use tessera_text::variables::Marker;
        let (doc, folio, _) = a_master_folio(3, &Marker::PageNumber.character().to_string());
        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        let folios: Vec<&ResolvedItem> =
            resolved.items.iter().filter(|i| i.frame == folio).collect();
        assert_eq!(folios.len(), 3, "one frame, three pages");
        let pages: Vec<PageId> = doc.page_ids().collect();
        for (n, page) in pages.iter().enumerate() {
            let on_page = folios
                .iter()
                .find(|i| i.on == Some(*page))
                .expect("resolved for the page");
            assert_eq!(
                glyphs_of(on_page),
                glyphs_for(&mut shaper, &(n + 1).to_string()),
                "page {} reads its own number",
                n + 1
            );
        }
    }

    #[test]
    fn a_section_restarts_the_count_in_its_own_style() {
        use tessera_document::sections::Section;
        use tessera_text::story::Numbering;
        use tessera_text::variables::Marker;
        let (mut doc, folio, _) = a_master_folio(3, &Marker::PageNumber.character().to_string());
        let pages: Vec<PageId> = doc.page_ids().collect();
        doc.set_sections(vec![Section {
            first: pages[1],
            start: Some(1),
            style: Numbering::LowerRoman,
            prefix: String::new(),
            marker: String::new(),
        }]);
        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);
        let on = |page: PageId| {
            resolved
                .items
                .iter()
                .find(|i| i.frame == folio && i.on == Some(page))
                .map(glyphs_of)
                .expect("resolved")
        };
        assert_eq!(on(pages[0]), glyphs_for(&mut shaper, "1"));
        assert_eq!(on(pages[1]), glyphs_for(&mut shaper, "i"));
        assert_eq!(on(pages[2]), glyphs_for(&mut shaper, "ii"));
    }

    #[test]
    fn on_the_parent_itself_the_folio_reads_the_parents_prefix() {
        use tessera_text::variables::Marker;
        let (doc, folio, master) = a_master_folio(1, &Marker::PageNumber.character().to_string());
        let mut shaper = Shaper::new();
        let resolved = resolve_scope(&doc, &mut shaper, Scope::Master(master));
        let item = resolved
            .items
            .iter()
            .find(|i| i.frame == folio)
            .expect("the parent shows it");
        assert_eq!(glyphs_of(item), glyphs_for(&mut shaper, "A"));
    }

    #[test]
    fn a_running_header_reads_the_pages_heading() {
        use tessera_document::variables::{TextVariable, Which};
        use tessera_text::story::{ParagraphFormat, ParagraphStyle};
        use tessera_text::variables::Marker;
        let (mut doc, folio, _) = a_master_folio(2, &Marker::Variable(0).character().to_string());
        let heading = doc.add_paragraph_style(ParagraphStyle {
            name: "Heading".into(),
            based_on: None,
            format: ParagraphFormat::default(),
        });
        doc.set_variables(vec![TextVariable::running_header(
            "Chapter",
            heading,
            Which::First,
        )]);

        // Page two carries a heading; page one carries nothing in that style.
        let pages: Vec<PageId> = doc.page_ids().collect();
        let bounds = doc.pages[pages[1]].bounds;
        let layer = doc.default_layer().expect("layer");
        let mut story = Story::new(
            "Alpha
Some body copy.",
        );
        story.set_paragraph_style(0..6, Some(heading));
        let story = doc.add_story(story);
        doc.add_frame(layer, {
            let mut f = rect(bounds.x + 10.0, bounds.y + 100.0, 300.0, 200.0);
            f.kind = FrameKind::text(story);
            f
        });

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);
        let on = |page: PageId| {
            resolved
                .items
                .iter()
                .find(|i| i.frame == folio && i.on == Some(page))
                .map(glyphs_of)
                .expect("resolved")
        };
        assert_eq!(on(pages[1]), glyphs_for(&mut shaper, "Alpha"));
        assert!(
            on(pages[0]).is_empty(),
            "no heading on page one, so nothing to say"
        );
    }

    #[test]
    fn a_footnote_is_set_at_the_foot_of_the_frame_that_cites_it() {
        use tessera_text::variables::Marker;
        let mut doc = Document::default();
        let page = doc.page_ids().next().expect("a page");
        let layer = doc.default_layer().expect("a layer");
        let bounds = doc.pages[page].bounds;
        let mut story = Story::new(format!(
            "A claim{} and more copy.",
            Marker::FootnoteReference.character()
        ));
        let end = story.footnotes[0].text.len();
        story.footnotes[0].insert_text(end, "The source.");
        let story = doc.add_story(story);
        let host = doc.add_frame(layer, {
            let mut f = rect(bounds.x + 20.0, bounds.y + 20.0, 300.0, 300.0);
            f.kind = FrameKind::text(story);
            f
        });

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);
        let item = item_for(&resolved, host).expect("resolved");
        let ResolvedKind::Text { shaped, .. } = &item.kind else {
            panic!("text");
        };
        let notes: Vec<_> = shaped.lines.iter().filter(|l| l.range.is_empty()).collect();
        let body: Vec<_> = shaped
            .lines
            .iter()
            .filter(|l| !l.range.is_empty())
            .collect();
        assert_eq!(notes.len(), 1, "one note, one line");
        assert!(notes[0].hit.is_none(), "a caret cannot get into it");
        assert!(
            notes[0].baseline > body[0].baseline,
            "the note sits below the copy"
        );
        assert!(
            notes[0].baseline + notes[0].descent <= 300.0 + 1e-6,
            "and inside the frame"
        );
        // Numbered: the note's first glyph is the figure 1, the same glyph
        // as the reference's.
        let reference_glyph = body[0]
            .runs
            .iter()
            .find(|r| r.size < 12.0)
            .map(|r| r.glyphs[0].glyph_id);
        let note_glyph = notes[0].runs[0].glyphs.first().map(|g| g.glyph_id);
        assert_eq!(
            reference_glyph, note_glyph,
            "the note leads with the number it is cited by"
        );
        assert_eq!(item.on, Some(page));
    }

    #[test]
    fn footnotes_count_as_the_options_say_and_restart_per_page() {
        use tessera_document::footnotes::{FootnoteNumbering, FootnoteOptions, Restart};
        use tessera_text::variables::Marker;
        // Two frames threaded across two pages, one note in each.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let second = doc.add_page();
        let first = doc.page_ids().next().unwrap();
        let layer = doc.default_layer().unwrap();
        let r = Marker::FootnoteReference.character();
        let text = format!("{}{r}\n{}{r}", "word ".repeat(12), "more ".repeat(12));
        let story = doc.add_story(Story::new(text));
        let a = doc.add_frame(layer, {
            let b = doc.pages[first].bounds;
            // Room for the first paragraph and its note, not the second.
            let mut f = rect(b.x + 20.0, b.y + 20.0, 200.0, 90.0);
            f.kind = FrameKind::text(story);
            f
        });
        let b = doc.add_frame(layer, {
            let bb = doc.pages[second].bounds;
            let mut f = rect(bb.x + 20.0, bb.y + 20.0, 200.0, 400.0);
            f.kind = FrameKind::text(story);
            f
        });
        assert!(doc.thread(a, b));
        doc.set_footnote_options(FootnoteOptions {
            numbering: FootnoteNumbering::Symbols,
            restart: Restart::Page,
            ..Default::default()
        });

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);
        let reference_glyph = |frame: FrameId| -> Option<u32> {
            let item = item_for(&resolved, frame)?;
            let ResolvedKind::Text { shaped, .. } = &item.kind else {
                return None;
            };
            shaped
                .lines
                .iter()
                .filter(|l| !l.range.is_empty())
                .flat_map(|l| l.runs.iter())
                .find(|r| r.size < 12.0)
                .and_then(|r| r.glyphs.first().map(|g| g.glyph_id))
        };
        let star = glyphs_for(&mut shaper, "*");
        assert_eq!(
            reference_glyph(a),
            star.first().copied(),
            "the first note is *"
        );
        assert_eq!(
            reference_glyph(b),
            star.first().copied(),
            "and so is the first note on the next page, restarted"
        );

        doc.set_footnote_options(FootnoteOptions {
            numbering: FootnoteNumbering::Symbols,
            restart: Restart::Never,
            ..Default::default()
        });
        let resolved = resolve(&doc, &mut shaper);
        let reference_glyph = |frame: FrameId| -> Option<u32> {
            let item = item_for(&resolved, frame)?;
            let ResolvedKind::Text { shaped, .. } = &item.kind else {
                return None;
            };
            shaped
                .lines
                .iter()
                .filter(|l| !l.range.is_empty())
                .flat_map(|l| l.runs.iter())
                .find(|r| r.size < 12.0)
                .and_then(|r| r.glyphs.first().map(|g| g.glyph_id))
        };
        let dagger = glyphs_for(&mut shaper, "\u{2020}");
        assert_eq!(
            reference_glyph(b),
            dagger.first().copied(),
            "counting on: \u{2020}"
        );
    }

    #[test]
    fn a_shape_wrap_takes_less_room_at_the_shoulder_than_at_the_waist() {
        use tessera_document::nodes::TextWrap;
        // A wide frame of many short lines, and an ellipse standing at its
        // left edge, level with the first lines.
        let mut doc = Document::default();
        let page = doc.page_ids().next().expect("a page");
        let layer = doc.default_layer().expect("a layer");
        let bounds = doc.pages[page].bounds;
        let words = "word ".repeat(200);
        let story = doc.add_story(Story::new(words));
        let host = doc.add_frame(layer, {
            let mut f = rect(bounds.x + 20.0, bounds.y + 20.0, 400.0, 400.0);
            f.kind = FrameKind::text(story);
            f
        });
        let mut oval = rect(bounds.x + 20.0, bounds.y + 20.0, 120.0, 120.0);
        oval.kind = FrameKind::Ellipse;
        oval.wrap = TextWrap::Contour {
            standoff: 0.0,
            sides: Default::default(),
        };
        let oval = doc.add_frame(layer, oval);

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);
        let item = item_for(&resolved, host).expect("resolved");
        let ResolvedKind::Text { shaped, .. } = &item.kind else {
            panic!("text");
        };
        // Where each line starts: the first glyph's x.
        let starts: Vec<f64> = shaped
            .lines
            .iter()
            .filter_map(|l| l.glyphs().next().map(|g| g.x))
            .collect();
        assert!(starts.len() > 6, "enough lines: {}", starts.len());
        // The line nearest the ellipse's middle (y ≈ 60) starts furthest
        // right; the first line, at its shoulder, starts further left; the
        // lines below it start at the margin.
        let waist = shaped
            .lines
            .iter()
            .min_by(|a, b| {
                (a.baseline - 60.0)
                    .abs()
                    .total_cmp(&(b.baseline - 60.0).abs())
            })
            .and_then(|l| l.glyphs().next().map(|g| g.x))
            .expect("a line at the waist");
        assert!(waist > starts[0], "waist {waist} vs shoulder {}", starts[0]);
        assert!(starts[0] > 0.0, "the shoulder still pushes the first line");
        assert!(
            *starts.last().unwrap() < 1.0,
            "below the ellipse, the margin"
        );

        // The box wrap, for contrast, pushes every crossing line the same.
        doc.frames[oval].wrap = TextWrap::Bounds {
            standoff: Default::default(),
            sides: Default::default(),
        };
        doc.touch();
        let resolved = resolve(&doc, &mut shaper);
        let item = item_for(&resolved, host).expect("resolved");
        let ResolvedKind::Text { shaped, .. } = &item.kind else {
            panic!("text");
        };
        let boxed: Vec<f64> = shaped
            .lines
            .iter()
            .take(2)
            .filter_map(|l| l.glyphs().next().map(|g| g.x))
            .collect();
        assert!((boxed[0] - boxed[1]).abs() < 1e-6, "the box: {boxed:?}");
    }

    /// The text a resolved text frame shows, joined line by line from the
    /// shaped text's glyph runs' stored ranges — what a reader would read.
    fn shown_text(resolved: &ResolvedDocument, _doc: &Document, frame: FrameId) -> String {
        let item = item_for(resolved, frame).expect("resolved");
        let ResolvedKind::Text { shaped, .. } = &item.kind else {
            panic!("text");
        };
        // The shaped text lives in the caret's line layouts; read it back
        // from the first line's paragraph, which holds the whole paragraph.
        shaped
            .lines
            .iter()
            .filter_map(|l| l.hit.as_ref())
            .map(|hit| hit.shaped_text().to_owned())
            .fold(Vec::<String>::new(), |mut acc, t| {
                if acc.last() != Some(&t) {
                    acc.push(t);
                }
                acc
            })
            .join(
                "
",
            )
    }

    #[test]
    fn a_cross_reference_reads_as_the_page_and_paragraph_its_anchor_is_on() {
        use tessera_text::story::{CrossReference, CrossReferenceFormat, TextAnchor};
        use tessera_text::variables::Marker;
        let mut doc = Document::default();
        let second = doc.add_page();
        let first = doc.page_ids().next().unwrap();
        let layer = doc.default_layer().expect("a layer");

        // Page one: "See ⟨ref⟩." Page two: "⟨anchor⟩Chapter Two / Body."
        let mut referring = Story::new(format!("See {}.", Marker::CrossReference.character()));
        referring.cross_references[0] = CrossReference {
            target: "ch2".into(),
            format: CrossReferenceFormat::ParagraphAndPage,
        };
        let referring = doc.add_story(referring);
        let mut target = Story::new(format!(
            "{}Chapter Two
The body of the chapter.",
            Marker::TextAnchor.character()
        ));
        target.anchors[0] = TextAnchor { name: "ch2".into() };
        let target = doc.add_story(target);

        let a = doc.add_frame(layer, {
            let b = doc.pages[first].bounds;
            let mut f = rect(b.x + 20.0, b.y + 20.0, 300.0, 60.0);
            f.kind = FrameKind::text(referring);
            f
        });
        doc.add_frame(layer, {
            let b = doc.pages[second].bounds;
            let mut f = rect(b.x + 20.0, b.y + 20.0, 300.0, 200.0);
            f.kind = FrameKind::text(target);
            f
        });

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);
        let shown = shown_text(&resolved, &doc, a);
        assert_eq!(shown, "See Chapter Two on page 2.", "{shown}");

        // Just the page, just the paragraph.
        doc.stories[referring].cross_references[0].format = CrossReferenceFormat::PageNumber;
        doc.touch();
        assert_eq!(shown_text(&resolve(&doc, &mut shaper), &doc, a), "See 2.");
        doc.stories[referring].cross_references[0].format = CrossReferenceFormat::ParagraphText;
        doc.touch();
        assert_eq!(
            shown_text(&resolve(&doc, &mut shaper), &doc, a),
            "See Chapter Two."
        );

        // A target nothing has: a question mark, not nothing.
        doc.stories[referring].cross_references[0].target = "nowhere".into();
        doc.touch();
        assert_eq!(shown_text(&resolve(&doc, &mut shaper), &doc, a), "See ?.");

        // A named page destination is a target too, for the page.
        doc.destinations
            .push(tessera_document::contents::Destination {
                name: "end".into(),
                page: second,
            });
        doc.stories[referring].cross_references[0] = CrossReference {
            target: "end".into(),
            format: CrossReferenceFormat::PageNumber,
        };
        doc.touch();
        assert_eq!(shown_text(&resolve(&doc, &mut shaper), &doc, a), "See 2.");
    }

    #[test]
    fn a_picture_wrapped_to_both_sides_has_text_either_side_of_it() {
        use tessera_document::nodes::{TextWrap, WrapTo};
        // A wide frame and a picture standing in the middle of its first
        // lines. Wrapped to the largest area the text runs down one side;
        // wrapped to both sides it runs down both, on the same baselines.
        let mut doc = Document::default();
        let page = doc.page_ids().next().expect("a page");
        let layer = doc.default_layer().expect("a layer");
        let bounds = doc.pages[page].bounds;
        let words = "word ".repeat(300);
        let story = doc.add_story(Story::new(words));
        let host = doc.add_frame(layer, {
            let mut f = rect(bounds.x + 20.0, bounds.y + 20.0, 400.0, 400.0);
            f.kind = FrameKind::text(story);
            f
        });
        let mut picture = rect(bounds.x + 20.0 + 150.0, bounds.y + 20.0, 100.0, 100.0);
        picture.wrap = TextWrap::Bounds {
            standoff: Default::default(),
            sides: WrapTo::Both,
        };
        let picture = doc.add_frame(layer, picture);

        let mut shaper = Shaper::new();
        let lines_of = |doc: &Document, shaper: &mut Shaper| {
            let resolved = resolve(doc, shaper);
            let item = item_for(&resolved, host).expect("resolved");
            let ResolvedKind::Text { shaped, .. } = &item.kind else {
                panic!("text");
            };
            shaped.lines.clone()
        };
        let both = lines_of(&doc, &mut shaper);
        // Rows the picture crosses hold two lines each, one either side.
        let crossing: Vec<_> = both.iter().filter(|l| l.baseline < 100.0).collect();
        let paired = crossing
            .windows(2)
            .filter(|w| (w[0].baseline - w[1].baseline).abs() < 1e-6)
            .count();
        assert!(paired >= 3, "{paired} pairs among {} lines", crossing.len());
        for w in crossing.windows(2) {
            if (w[0].baseline - w[1].baseline).abs() < 1e-6 {
                let left = w[0].glyphs().map(|g| g.x).fold(f64::MIN, f64::max);
                let right = w[1].glyphs().map(|g| g.x).fold(f64::MAX, f64::min);
                assert!(left < 150.0 && right >= 250.0, "{left} | {right}");
            }
        }

        // The largest area, for contrast: no two lines share a baseline.
        doc.frames[picture].wrap = TextWrap::Bounds {
            standoff: Default::default(),
            sides: WrapTo::Largest,
        };
        doc.touch();
        let largest = lines_of(&doc, &mut shaper);
        assert!(
            largest
                .windows(2)
                .all(|w| (w[0].baseline - w[1].baseline).abs() > 1e-6),
            "one line per row"
        );
        assert!(
            both.len() > largest.len(),
            "both sides takes more, shorter lines"
        );
    }

    #[test]
    fn a_master_item_is_drawn_behind_the_page_it_is_on() {
        // A master carries the furniture a page is laid out on top of.
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let master_bounds = doc.pages[on].bounds;
        let layer = doc.default_layer().expect("layer");
        let furniture = doc.add_frame(
            layer,
            rect(master_bounds.x + 10.0, master_bounds.y + 10.0, 20.0, 20.0),
        );

        let page = doc.page_ids().next().expect("a page");
        let target = doc.pages[page].bounds;
        let own = doc.add_frame(layer, rect(target.x + 40.0, target.y + 40.0, 20.0, 20.0));
        doc.apply_master(page, Some(master));

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        // On the page itself: the inherited furniture is resolved before the
        // walk, so it precedes everything the page holds of its own.
        let order: Vec<_> = resolved.items.iter().map(|i| i.frame).collect();
        let inherited = order.iter().position(|f| *f == furniture).expect("drawn");
        let mine = order.iter().position(|f| *f == own).expect("drawn");
        assert!(inherited < mine, "the master first, then the page");
    }

    #[test]
    fn an_overridden_item_is_drawn_once() {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let master = doc.add_master("A-Master");
        let on = doc.pages_of_master(master)[0];
        let bounds = doc.pages[on].bounds;
        let layer = doc.default_layer().expect("layer");
        let item = doc.add_frame(layer, rect(bounds.x + 10.0, bounds.y + 10.0, 20.0, 20.0));

        let page = doc.page_ids().next().expect("a page");
        doc.apply_master(page, Some(master));
        let local = doc.override_master_item(page, item).expect("a copy");

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        assert_eq!(resolved.items.len(), 1, "not doubled");
        assert_eq!(resolved.items[0].frame, local, "the local copy stands in");
        assert!(
            !resolved.items.iter().any(|i| i.frame == item),
            "and the parent's own item is not drawn on the document"
        );
    }

    // --- threaded text ------------------------------------------------------

    /// Two text frames holding one long story, threaded.
    fn a_thread(first_height: f64) -> (Document, FrameId, FrameId) {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        let layer = doc.default_layer().expect("layer");

        let long = "one two three four five six seven eight nine ten \
                    eleven twelve thirteen fourteen fifteen sixteen"
            .to_string();
        let story = doc.add_story(tessera_text::story::Story::new(&long));

        let mut a = rect(0.0, 0.0, 90.0, first_height);
        a.kind = FrameKind::text(story);
        let a = doc.add_frame(layer, a);

        let spare = doc.add_story(tessera_text::story::Story::new(""));
        let mut b = rect(120.0, 0.0, 90.0, 400.0);
        b.kind = FrameKind::text(spare);
        let b = doc.add_frame(layer, b);

        doc.thread(a, b);
        (doc, a, b)
    }

    /// Which lines of the story a frame ended up drawing.
    fn drawn(resolved: &ResolvedDocument, frame: FrameId) -> usize {
        resolved
            .items
            .iter()
            .filter(|i| i.frame == frame)
            .map(|i| match &i.kind {
                ResolvedKind::Text { shaped, .. } => shaped.lines.len(),
                _ => 0,
            })
            .sum()
    }

    #[test]
    fn a_story_too_long_for_its_frame_continues_in_the_next() {
        let (doc, a, b) = a_thread(40.0);
        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        assert!(drawn(&resolved, a) > 0, "the first frame holds some");
        assert!(drawn(&resolved, b) > 0, "and the second holds the rest");
    }

    #[test]
    fn the_second_frame_does_not_repeat_the_first() {
        // The bug this arrangement exists to prevent: a second frame that
        // starts at zero shows the same opening lines again.
        let (doc, a, b) = a_thread(40.0);
        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        let text_of = |frame: FrameId| -> Vec<std::ops::Range<usize>> {
            resolved
                .items
                .iter()
                .filter(|i| i.frame == frame)
                .flat_map(|i| match &i.kind {
                    ResolvedKind::Text { shaped, .. } => {
                        shaped.lines.iter().map(|l| l.range.clone()).collect()
                    }
                    _ => Vec::new(),
                })
                .collect()
        };

        let first = text_of(a);
        let second = text_of(b);
        let ended = first.last().expect("lines").end;
        let began = second.first().expect("lines").start;

        assert!(
            began >= ended,
            "the second frame begins where the first stopped: {ended} then {began}"
        );
    }

    #[test]
    fn resizing_the_first_frame_reflows_the_chain() {
        // Milestone 4's sentence: resize the first, and watch the text reflow
        // through the chain.
        let mut shaper = Shaper::new();

        let (short, a, _) = a_thread(30.0);
        let held_when_short = drawn(&resolve(&short, &mut shaper), a);

        let (tall, a, b) = a_thread(200.0);
        let resolved = resolve(&tall, &mut shaper);
        let held_when_tall = drawn(&resolved, a);

        assert!(
            held_when_tall > held_when_short,
            "a taller first frame holds more: {held_when_tall} against {held_when_short}"
        );
        // And what it holds, the second one does not.
        assert!(drawn(&resolved, b) > 0 || held_when_tall > 0);
    }

    #[test]
    fn an_unthreaded_frame_lays_its_own_story_out_from_the_start() {
        let (mut doc, a, b) = a_thread(40.0);
        doc.unthread(a);

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        let second: Vec<std::ops::Range<usize>> = resolved
            .items
            .iter()
            .filter(|i| i.frame == b)
            .flat_map(|i| match &i.kind {
                ResolvedKind::Text { shaped, .. } => {
                    shaped.lines.iter().map(|l| l.range.clone()).collect()
                }
                _ => Vec::new(),
            })
            .collect();

        assert_eq!(
            second.first().map(|r| r.start),
            Some(0),
            "on its own again, it starts at the beginning"
        );
        assert!(drawn(&resolved, a) > 0);
    }
}

#[cfg(test)]
mod page_tests {
    use super::*;
    use tessera_document::nodes::{Insets, Margins};

    #[test]
    fn resolving_carries_every_pages_rectangles() {
        let mut doc = Document::new();
        doc.setup.margins = Margins::uniform(36.0);
        doc.setup.bleed = Insets::uniform(9.0);

        let mut shaper = Shaper::new();
        let resolved = resolve(&doc, &mut shaper);

        assert_eq!(resolved.pages.len(), doc.page_ids().count());
        let page = &resolved.pages[0];
        assert_eq!(page.margins.width, page.bounds.width - 72.0);
        assert_eq!(page.bleed.width, page.bounds.width + 18.0);
        assert_eq!(page.slug, page.bounds, "no slug set means no slug drawn");
    }
}
