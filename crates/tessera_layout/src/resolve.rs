//! Resolves a document into drawable items.
//!
//! Both the screen renderer and the PDF writer consume the output of this
//! module, so neither re-derives geometry nor re-shapes text. That shared
//! source is what keeps an export from drifting away from the screen.

use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::ids::FrameId;
use tessera_document::nodes::{FrameKind, Stroke};
use tessera_document::path::fit_to_bounds;
use tessera_geometry::{DocRect, Transform};
use tessera_text::shape::{ShapedText, Shaper};

pub use tessera_document::document::StoryMap;

#[derive(Debug, Clone)]
pub enum ResolvedKind {
    Rectangle {
        fill: Color,
        stroke: Option<Stroke>,
    },
    Ellipse {
        fill: Color,
        stroke: Option<Stroke>,
    },
    Text {
        shaped: ShapedText,
        color: Color,
    },
    /// A path in frame-local coordinates. Consumers translate by
    /// [`ResolvedItem::bounds`]'s origin.
    Path {
        path: kurbo::BezPath,
        fill: Option<Color>,
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPage {
    /// The trim: the paper itself.
    pub bounds: DocRect,
    /// The type area, inset by the margins.
    pub margins: DocRect,
    /// The trim plus its bleed.
    pub bleed: DocRect,
    /// The trim plus its slug.
    pub slug: DocRect,
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

/// Resolve what `scope` is looking at.
pub fn resolve_scope(doc: &Document, shaper: &mut Shaper, scope: Scope) -> ResolvedDocument {
    let shown: Vec<tessera_document::ids::PageId> = match scope {
        Scope::Document => doc.page_ids().collect(),
        Scope::Master(id) => doc.pages_of_master(id),
    };
    resolve_pages(doc, shaper, &shown)
}

/// Resolve exactly these pages, and what they inherit.
///
/// The scope has already been turned into a list of pages by the time this
/// runs, which is the whole of the difference between looking at the document
/// and looking at one parent.
fn resolve_pages(
    doc: &Document,
    shaper: &mut Shaper,
    shown: &[tessera_document::ids::PageId],
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
                let Some(mut resolved) = resolve_one(doc, shaper, leaf, frame) else {
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
        if let Some(item) = resolve_one(doc, shaper, id, frame) {
            items.push(item);
        }
    }

    ResolvedDocument { items, pages }
}

/// One frame, resolved.
///
/// Pulled out of the walk so that a master's item can be resolved the same way
/// a page's own is — the difference between them is where it lands, not what
/// it is. `None` for a frame that draws nothing: a group, or a text frame
/// whose story has gone.
fn resolve_one(
    doc: &Document,
    shaper: &mut Shaper,
    id: FrameId,
    frame: &tessera_document::nodes::Frame,
) -> Option<ResolvedItem> {
    let kind = match &frame.kind {
        FrameKind::Rectangle => ResolvedKind::Rectangle {
            fill: frame.fill.clone(),
            stroke: frame.stroke.clone(),
        },
        FrameKind::Ellipse => ResolvedKind::Ellipse {
            fill: frame.fill.clone(),
            stroke: frame.stroke.clone(),
        },
        FrameKind::Path(path) => ResolvedKind::Path {
            path: fit_to_bounds(path, frame.bounds),
            // An open path with no explicit stroke would be invisible, so
            // a path frame's fill is treated as its stroke colour when it
            // has no stroke of its own.
            fill: None,
            stroke: Some(
                frame
                    .stroke
                    .clone()
                    .unwrap_or_else(|| Stroke::new(frame.fill.clone(), 1.0)),
            ),
        },

        // A group draws nothing of its own, and paint_order already
        // expanded it into its children, so it never reaches here.
        FrameKind::Group(_) => return None,

        FrameKind::Text { story } => {
            // A text frame whose story is missing is a broken document,
            // not a blank frame. Skipping it silently would hide the
            // breakage; milestone 6's preflight reports it. For now it
            // simply does not paint, which is visible.
            let story = doc.story(*story)?;
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
            ResolvedKind::Text {
                shaped: shaper.shape(story, doc, frame.bounds.width),
                color: colour,
            }
        }
    };

    Some(ResolvedItem {
        frame: id,
        bounds: frame.bounds,
        transform: frame.transform,
        spread_area: doc.spread_of_frame(id).and_then(|s| doc.spread_area(s)),
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
            fill: Color::BLACK,
            stroke: None,
        }
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
        frame.kind = FrameKind::Text { story };
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
        frame.kind = FrameKind::Text { story };
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
        frame.kind = FrameKind::Text { story };
        doc.add_frame(layer, frame);

        assert!(resolve(&doc, &mut Shaper::new()).items.is_empty());
    }

    fn path_frame(bounds: DocRect, path: kurbo::BezPath) -> Frame {
        Frame {
            bounds,
            kind: FrameKind::Path(path),
            transform: Transform::IDENTITY,
            fill: Color::BLACK,
            stroke: None,
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
        let page = resolved.pages[0];
        assert_eq!(page.margins.width, page.bounds.width - 72.0);
        assert_eq!(page.bleed.width, page.bounds.width + 18.0);
        assert_eq!(page.slug, page.bounds, "no slug set means no slug drawn");
    }
}
