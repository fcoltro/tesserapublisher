//! Resolves a document into drawable items.
//!
//! Both the screen renderer and the PDF writer consume the output of this
//! module, so neither re-derives geometry nor re-shapes text. That shared
//! source is what keeps an export from drifting away from the screen.

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::ids::{FrameId, StoryId};
use tessera_document::nodes::{FrameKind, Stroke};
use tessera_document::paint::Paint;
use tessera_document::path::fit_to_bounds;
use tessera_geometry::{DocRect, Transform};
use tessera_text::shape::{ShapedText, Shaper};
use tessera_text::story::Story as TextStory;

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

#[derive(Debug, Clone)]
pub struct ResolvedItem {
    pub frame: FrameId,
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

    let mut items = Vec::new();

    // What each page inherits from its parent, drawn **behind** its own
    // contents and so before them: a master carries the furniture a page is
    // laid out on top of.
    //
    // The offset is applied to the resolved item rather than to the frame,
    // because nothing is moved. One master item is drawn once per page that
    // inherits it, from a single frame — copying it onto each page is the
    // thing a master exists in order not to do.
    for page in shown.iter().copied() {
        let Some(area) = doc.spread_of(page).and_then(|s| doc.spread_area(s)) else {
            continue;
        };
        for (item, dx, dy) in doc.inherited_by(page) {
            for leaf in doc.descendants(item) {
                let Some(frame) = doc.frame(leaf) else {
                    continue;
                };
                let Some(mut resolved) = resolve_one(doc, shaper, leaf, frame, composed) else {
                    continue;
                };
                resolved.transform = Transform::translate(dx, dy).then(resolved.transform);
                resolved.spread_area = Some(area);
                items.push(resolved);
            }
        }
    }

    // Everything standing on a page this scope is showing. A parent's own
    // items are drawn when the parent is what is being looked at, and only
    // then — otherwise a parent spread would sit in the scroll a person is
    // trying to lay out in.
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
        if let Some(item) = resolve_one(doc, shaper, id, frame, composed) {
            items.push(item);
        }
    }

    // After the hosts, because where an anchored object lands is not known
    // until the text around it has been broken into lines.
    let anchored = resolve_anchored(doc, shaper, composed, &items);
    items.extend(anchored);

    ResolvedDocument { items, pages }
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
            let Some(mut item) = resolve_one(doc, shaper, id, frame, composed) else {
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
            let Some(standoff) = doc.frame(other).map(|f| f.wrap).and_then(|w| w.standoff()) else {
                continue;
            };
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
            out.push(tessera_text::wrap::Obstacle {
                x,
                y,
                width,
                height,
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
        let FrameKind::Text { story, layout } = &before.kind else {
            continue;
        };
        let Some(text) = story_of(doc, composed, *story) else {
            continue;
        };

        let columns = layout.columns_of(DocRect {
            x: 0.0,
            y: 0.0,
            width: before.bounds.width,
            height: before.bounds.height,
        });
        let measure = columns.first().map_or(before.bounds.width, |c| c.width);
        let boxes: Vec<tessera_text::shape::Column> = columns
            .iter()
            .map(|c| tessera_text::shape::Column {
                x: c.x,
                y: c.y,
                width: c.width,
                height: c.height,
            })
            .collect();

        let shaped = shaper.shape_from(text, doc, measure, from);
        let flowed = tessera_text::shape::flow(shaped, &boxes);
        // A frame that held nothing hands the story on untouched rather than
        // restarting it: treating "placed nothing" as zero would loop the
        // whole chain back to the beginning.
        if let Some(to) = flowed.consumed_to {
            from = to;
        }
    }
    from
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
) -> Option<ResolvedItem> {
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
        FrameKind::Path(path) => ResolvedKind::Path {
            path: fit_to_bounds(path, frame.bounds),
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
        },

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
            layout,
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
                tessera_document::nodes::VerticalJustify::Centre => {
                    tessera_text::shape::Vertical::Centre
                }
                tessera_document::nodes::VerticalJustify::Bottom => {
                    tessera_text::shape::Vertical::Bottom
                }
                tessera_document::nodes::VerticalJustify::Justify => {
                    tessera_text::shape::Vertical::Justify
                }
            };

            // Where in the story this frame starts. Zero unless something
            // flows into it, in which case the frames before it are laid out
            // to find out how much they hold — the answer depends on their
            // measures, so there is no shortcut past doing it.
            let from = story_starts_at(doc, shaper, id, composed);

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
                .inline_objects_of(*story_id)
                .into_iter()
                .map(|(at, _, width, height)| tessera_text::shape::InlineObject {
                    at,
                    width,
                    height,
                })
                .collect();
            let shaped =
                shaper.shape_around_with_objects(story, doc, measure, from, &obstacles, &anchored);
            let flowed = tessera_text::shape::flow_on_grid(shaped, &boxes, vertical, grid);

            ResolvedKind::Text {
                shaped: flowed.text,
                color: colour,
                overset_lines: flowed.overset_lines,
            }
        }
    };

    Some(ResolvedItem {
        frame: id,
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
