//! InDesign Markup Language: the package InDesign exports for interchange.
//!
//! A `.idml` is a zip of XML. `designmap.xml` is the spine: it names the
//! resource files, then every parent spread, spread and story in order, and
//! holds the sections and layers itself. The importer reads the spine, then
//! the resources it points at, then builds a [`Document`] in the order the
//! document depends on itself: colours before styles (a style names a
//! colour), styles before stories, stories before frames, frames before
//! threads.
//!
//! ## Where things are
//!
//! IDML places everything in **spread space**, with the origin at the
//! spread's centre and each page carrying its own transform into it. Tessera
//! lays spreads out for itself, so an item is not placed where IDML says: it
//! is placed *relative to the page it is on* — the page whose rectangle holds
//! its centre — and that offset is applied to wherever Tessera put the page.
//! An item hanging off every page goes with the first page of its spread,
//! which is where the pasteboard belongs.
//!
//! An item's transform is the same matrix IDML gives it, re-based so it turns
//! about the item's own origin in Tessera's space rather than the spread's.

mod story;
mod styles;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use roxmltree::Node;
use tessera_color::Color;
use tessera_document::document::Document;
use tessera_document::ids::{FrameId, LayerId, MasterId, PageId, StoryId};
use tessera_document::nodes::{
    Frame, FrameKind, Insets, Stroke, Swatch, TextLayout, VerticalJustify,
};
use tessera_document::paint::Paint;
use tessera_document::sections::Section;
use tessera_geometry::{DocRect, Transform};
use tessera_text::story::Numbering;

use crate::xml::{Package, attr, attr_f64, child, children, numbers, parse, property};
use crate::{Dropped, ImportError};
use styles::{Colours, Styles};

/// A document read from a package, and what could not come with it.
#[derive(Debug)]
pub struct Imported {
    pub document: Document,
    pub dropped: Dropped,
}

pub fn import(path: &Path) -> Result<Imported, ImportError> {
    let package = Package::open(path, "IDML")?;
    import_package(package)
}

/// The same, from bytes already in hand — what the tests use.
pub fn import_bytes(bytes: Vec<u8>, path: &Path) -> Result<Imported, ImportError> {
    let package = Package::from_bytes(bytes, path, "IDML")?;
    import_package(package)
}

fn import_package(mut package: Package) -> Result<Imported, ImportError> {
    let spine_text = package.text("designmap.xml")?;
    let spine = parse("designmap.xml", &spine_text)?;
    let root = spine.root_element();
    let mut dropped = Dropped::default();

    // What the spine points at, in the order it does.
    let srcs = |name: &str| -> Vec<String> {
        root.children()
            .filter(|n| n.is_element() && n.tag_name().name() == name)
            .filter_map(|n| attr(n, "src").map(str::to_owned))
            .collect()
    };
    let graphic = srcs("Graphic").into_iter().next();
    let style_src = srcs("Styles").into_iter().next();
    let preferences = srcs("Preferences").into_iter().next();
    let masters = srcs("MasterSpread");
    let spreads = srcs("Spread");
    let story_srcs = srcs("Story");

    let mut doc = Document::new();

    // Colours, then the swatch panel from them.
    let colours = match graphic {
        Some(src) if package.has(&src) => {
            let text = package.text(&src)?;
            let xml = parse(&src, &text)?;
            Colours::read(xml.root())
        }
        _ => Colours::default(),
    };
    for (name, colour) in colours.swatches() {
        doc.set_swatch(Swatch {
            name,
            colour,
            spot: false,
        });
    }

    // Page size and the like, before any page is added.
    if let Some(src) = preferences.filter(|s| package.has(s)) {
        let text = package.text(&src)?;
        let xml = parse(&src, &text)?;
        read_preferences(xml.root(), &mut doc);
    }

    let styles = match style_src {
        Some(src) if package.has(&src) => {
            let text = package.text(&src)?;
            let xml = parse(&src, &text)?;
            Styles::read(xml.root(), &mut doc, &colours)
        }
        _ => Styles::default(),
    };

    // Stories, by their Self. The files stay parsed until the end, because
    // an object set into a story's text is a node in the story's file, and
    // its frame is made only once every story and layer exists.
    let mut story_texts = Vec::new();
    for src in story_srcs {
        if !package.has(&src) {
            continue;
        }
        let text = package.text(&src)?;
        story_texts.push((src, text));
    }
    let mut story_xml = Vec::new();
    for (src, text) in &story_texts {
        story_xml.push(parse(src, text)?);
    }
    // Where cross-references point: the spine's hyperlinks from a source
    // to a destination, and every destination's name, wherever it sits —
    // before any story is read, since a reference may point forward.
    let mut links = story::Links::default();
    for hyperlink in root.descendants().filter(|n| is_plain(*n, "Hyperlink")) {
        let Some(source) = attr(hyperlink, "Source") else {
            continue;
        };
        let destination = hyperlink
            .children()
            .find(|n| is_plain(*n, "Destination"))
            .and_then(|n| n.text())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(destination) = destination {
            links
                .sources
                .insert(source.to_owned(), destination.to_owned());
        }
    }
    for xml in &story_xml {
        for node in xml.descendants().filter(|n| {
            is_plain(*n, "HyperlinkTextDestination") || is_plain(*n, "ParagraphDestination")
        }) {
            if let (Some(id), Some(name)) = (attr(node, "Self"), story::destination_name(node)) {
                links.destinations.insert(id.to_owned(), name);
            }
        }
    }

    let mut stories: HashMap<String, StoryId> = HashMap::new();
    let mut inline: Vec<(StoryId, usize, Node)> = Vec::new();
    for xml in &story_xml {
        for node in xml.descendants().filter(|n| is_plain(*n, "Story")) {
            let Some(name) = attr(node, "Self") else {
                continue;
            };
            let read = story::read(node, &styles, &colours, &links);
            let id = doc.add_story(read.story);
            stories.insert(name.to_owned(), id);
            for (index, item) in read.inline {
                inline.push((id, index, item));
            }
        }
    }

    // Layers, in the order the spine lists them (InDesign lists top first;
    // Tessera keeps bottom first).
    let mut layers: HashMap<String, LayerId> = HashMap::new();
    let listed: Vec<Node> = children(root, "Layer").collect();
    if !listed.is_empty() {
        for (i, node) in listed.iter().rev().enumerate() {
            let name = attr(*node, "Name").unwrap_or("Layer").to_owned();
            let id = if i == 0 {
                let id = doc.default_layer().expect("a new document has a layer");
                if let Some(layer) = doc.layers.get_mut(id) {
                    layer.name = name;
                }
                id
            } else {
                doc.add_layer(name)
            };
            if let Some(layer) = doc.layers.get_mut(id) {
                layer.visible = attr(*node, "Visible") != Some("false");
                layer.locked = attr(*node, "Locked") == Some("true");
            }
            if let Some(name) = attr(*node, "Self") {
                layers.insert(name.to_owned(), id);
            }
        }
    }
    let fallback_layer = doc.default_layer().expect("a layer");

    let mut items = Items {
        stories: &stories,
        layers: &layers,
        fallback_layer,
        colours: &colours,
        styles: &styles,
        frames: HashMap::new(),
        threads: Vec::new(),
        image_fit_noted: false,
    };

    // Parents first, so pages can be built on them.
    let mut master_ids: HashMap<String, MasterId> = HashMap::new();
    for src in masters {
        if !package.has(&src) {
            continue;
        }
        let text = package.text(&src)?;
        let xml = parse(&src, &text)?;
        let Some(spread) = xml.descendants().find(|n| is_plain(*n, "MasterSpread")) else {
            continue;
        };
        let name = attr(spread, "Name")
            .map(str::to_owned)
            .unwrap_or_else(|| doc.unused_master_name());
        let master = doc.add_master(name);
        if let Some(name) = attr(spread, "Self") {
            master_ids.insert(name.to_owned(), master);
        }
        let pages = doc.pages_of_master(master);
        let spread_pages = read_pages(spread);
        // A parent with more pages than Tessera's facing setting allows keeps
        // its first ones; the rest are said.
        if spread_pages.len() > pages.len() {
            dropped.note(format!(
                "parent {} has {} pages; only {} were kept",
                attr(spread, "Name").unwrap_or("?"),
                spread_pages.len(),
                pages.len()
            ));
        }
        let placed: Vec<(SpreadPage, PageId)> = spread_pages.into_iter().zip(pages).collect();
        items.place_all(spread, &placed, &mut doc, &mut dropped);
    }

    // Pages: count them, make them, map them in reading order.
    let mut spread_docs = Vec::new();
    for src in &spreads {
        if !package.has(src) {
            continue;
        }
        let text = package.text(src)?;
        spread_docs.push((src.clone(), text));
    }
    let mut parsed = Vec::new();
    for (src, text) in &spread_docs {
        parsed.push(parse(src, text)?);
    }
    let mut spread_nodes: Vec<(Node, Vec<SpreadPage>)> = Vec::new();
    for xml in &parsed {
        if let Some(spread) = xml.descendants().find(|n| is_plain(*n, "Spread")) {
            let pages = read_pages(spread);
            spread_nodes.push((spread, pages));
        }
    }
    let total: usize = spread_nodes
        .iter()
        .map(|(_, p)| p.len())
        .sum::<usize>()
        .max(1);
    while doc.page_ids().count() < total {
        doc.add_page();
    }
    let tessera_pages: Vec<PageId> = doc.page_ids().collect();
    let mut next = 0usize;
    let mut page_ids: HashMap<String, PageId> = HashMap::new();
    for (spread, pages) in &spread_nodes {
        let placed: Vec<(SpreadPage, PageId)> = pages
            .iter()
            .cloned()
            .zip(tessera_pages[next..].iter().copied())
            .collect();
        next += placed.len();
        for (page, id) in &placed {
            page_ids.insert(page.name.clone(), *id);
            if let Some(master) = page.master.as_deref().and_then(|m| master_ids.get(m)) {
                doc.apply_master(*id, Some(*master));
            }
        }
        items.place_all(*spread, &placed, &mut doc, &mut dropped);
    }

    // The objects set into the stories' text, anchored to their markers.
    for (story, index, node) in inline {
        items.place_inline(
            node,
            story,
            index,
            &mut doc,
            &mut dropped,
            &styles,
            &colours,
        );
    }

    // Margins: the first page's, since Tessera's are document-wide.
    if let Some((_, pages)) = spread_nodes.first()
        && let Some(page) = pages.first()
        && let Some(margins) = page.margins
    {
        let mut setup = doc.setup;
        setup.margins.top = margins[0];
        setup.margins.inside = margins[1];
        setup.margins.bottom = margins[2];
        setup.margins.outside = margins[3];
        doc.set_setup(setup);
    }

    // Threads, now that both ends exist.
    for (from, to) in std::mem::take(&mut items.threads) {
        if let (Some(from), Some(to)) = (items.frames.get(&from), items.frames.get(&to)) {
            doc.thread(*from, *to);
        }
    }

    // Sections.
    let mut sections = Vec::new();
    for node in children(root, "Section") {
        let Some(first) = attr(node, "PageStart").and_then(|p| page_ids.get(p)) else {
            continue;
        };
        let start = attr_f64(node, "PageNumberStart").map(|n| n as u32);
        let continues = attr(node, "ContinueNumbering") == Some("true");
        let style = match attr(node, "PageNumberStyle") {
            Some("LowerRoman") => Numbering::LowerRoman,
            Some("UpperRoman") => Numbering::UpperRoman,
            Some("LowerLetters") => Numbering::LowerAlpha,
            Some("UpperLetters") => Numbering::UpperAlpha,
            _ => Numbering::Arabic,
        };
        let prefix = attr(node, "SectionPrefix").unwrap_or("").to_owned();
        let marker = attr(node, "Marker").unwrap_or("").to_owned();
        let is_first = tessera_pages.first() == Some(first);
        let plain = style == Numbering::Arabic
            && prefix.is_empty()
            && marker.is_empty()
            && (continues || start == Some(1));
        if is_first && plain {
            continue; // the implied first section
        }
        sections.push(Section {
            first: *first,
            start: if continues { None } else { start.or(Some(1)) },
            style,
            prefix,
            // InDesign's own attribute for the same choice; absent, on.
            include_prefix: attr(node, "IncludeSectionPrefix") != Some("false"),
            marker,
        });
    }
    if !sections.is_empty() {
        doc.set_sections(sections);
    }

    if package.has("XML/BackingStory.xml") && spine_text.contains("Index") {
        dropped.note("index entries (the index is not imported yet)");
    }

    doc.touch();
    Ok(Imported {
        document: doc,
        dropped,
    })
}

/// An element in the document's own namespace, not the package's: `<Spread>`
/// rather than `<idPkg:Spread>`, which wraps it and has the same local name.
fn is_plain(node: Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace().is_none()
}

/// `DocumentPreference` and the bleed, into the setup.
fn read_preferences(root: Node, doc: &mut Document) {
    let Some(pref) = root
        .descendants()
        .find(|n| n.tag_name().name() == "DocumentPreference")
    else {
        return;
    };
    let mut setup = doc.setup;
    if let (Some(w), Some(h)) = (attr_f64(pref, "PageWidth"), attr_f64(pref, "PageHeight")) {
        setup.facing_pages = attr(pref, "FacingPages") == Some("true");
        doc.set_setup(setup);
        doc.set_page_size(w, h);
    } else {
        setup.facing_pages = attr(pref, "FacingPages") == Some("true");
    }
    let bleed = |name: &str| attr_f64(pref, name).unwrap_or(0.0);
    setup.bleed = Insets {
        top: bleed("DocumentBleedTopOffset"),
        bottom: bleed("DocumentBleedBottomOffset"),
        left: bleed("DocumentBleedInsideOrLeftOffset"),
        right: bleed("DocumentBleedOutsideOrRightOffset"),
    };
    let slug = |name: &str| attr_f64(pref, name).unwrap_or(0.0);
    setup.slug = Insets {
        top: slug("SlugTopOffset"),
        bottom: slug("SlugBottomOffset"),
        left: slug("SlugInsideOrLeftOffset"),
        right: slug("SlugRightOrOutsideOffset"),
    };
    doc.set_setup(setup);
}

/// A page as the spread describes it: its rectangle in spread space.
#[derive(Debug, Clone)]
struct SpreadPage {
    name: String,
    /// Left, top, width, height in spread space.
    rect: DocRect,
    master: Option<String>,
    /// Top, left, bottom, right.
    margins: Option<[f64; 4]>,
}

fn read_pages(spread: Node) -> Vec<SpreadPage> {
    children(spread, "Page")
        .filter_map(|page| {
            let name = attr(page, "Self")?.to_owned();
            let bounds = attr(page, "GeometricBounds").map(numbers)?;
            let [top, left, bottom, right] = bounds.as_slice() else {
                return None;
            };
            let transform = attr(page, "ItemTransform")
                .map(numbers)
                .filter(|t| t.len() == 6)
                .unwrap_or_else(|| vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            let rect = DocRect {
                x: left + transform[4],
                y: top + transform[5],
                width: right - left,
                height: bottom - top,
            };
            let master = attr(page, "AppliedMaster")
                .filter(|m| *m != "n")
                .map(str::to_owned);
            let margins = child(page, "MarginPreference").map(|m| {
                [
                    attr_f64(m, "Top").unwrap_or(36.0),
                    attr_f64(m, "Left").unwrap_or(36.0),
                    attr_f64(m, "Bottom").unwrap_or(36.0),
                    attr_f64(m, "Right").unwrap_or(36.0),
                ]
            });
            Some(SpreadPage {
                name,
                rect,
                master,
                margins,
            })
        })
        .collect()
}

/// The frame-building state that outlives one spread.
struct Items<'a> {
    stories: &'a HashMap<String, StoryId>,
    layers: &'a HashMap<String, LayerId>,
    fallback_layer: LayerId,
    colours: &'a Colours,
    styles: &'a Styles,
    /// Every frame made, by its IDML Self — for threads.
    frames: HashMap<String, FrameId>,
    /// `(from, to)` by IDML Self, resolved once every frame exists.
    threads: Vec<(String, String)>,
    image_fit_noted: bool,
}

impl Items<'_> {
    /// Every item directly on the spread, and those inside its groups.
    fn place_all(
        &mut self,
        spread: Node,
        pages: &[(SpreadPage, PageId)],
        doc: &mut Document,
        dropped: &mut Dropped,
    ) {
        for node in spread.children().filter(|n| n.is_element()) {
            self.place(node, pages, doc, dropped, Transform::IDENTITY, None);
        }
    }

    /// An object set into a story's text: a frame anchored to the story's
    /// `index`th marker. A table becomes a table frame; a group its first
    /// member, since one marker anchors one frame.
    #[allow(clippy::too_many_arguments)]
    fn place_inline(
        &mut self,
        node: Node,
        story: StoryId,
        index: usize,
        doc: &mut Document,
        dropped: &mut Dropped,
        styles: &Styles,
        colours: &Colours,
    ) {
        let name = node.tag_name().name();
        let node = if name == "Group" {
            dropped.note("a group anchored in text (only its first member was kept)");
            let Some(first) = node.children().find(|n| n.is_element()) else {
                return;
            };
            first
        } else {
            node
        };
        if node.tag_name().name() == "Table" {
            self.place_table(node, story, index, doc, dropped, styles, colours);
            return;
        }
        self.place(
            node,
            &[],
            doc,
            dropped,
            Transform::IDENTITY,
            Some((story, index)),
        );
    }

    /// An IDML table into the table model, anchored in its story.
    #[allow(clippy::too_many_arguments)]
    fn place_table(
        &mut self,
        node: Node,
        story: StoryId,
        index: usize,
        doc: &mut Document,
        dropped: &mut Dropped,
        styles: &Styles,
        colours: &Colours,
    ) {
        use tessera_document::table::{Slot, Span};
        let widths: Vec<f64> = children(node, "Column")
            .map(|c| attr_f64(c, "SingleColumnWidth").unwrap_or(72.0))
            .collect();
        let heights: Vec<f64> = children(node, "Row")
            .map(|r| attr_f64(r, "SingleRowHeight").unwrap_or(12.0))
            .collect();
        let columns = widths.len().max(1);
        let rows = heights.len().max(1);
        let width: f64 = widths.iter().sum::<f64>().max(1.0);
        let mut table = tessera_document::table::new(rows, columns, width, || {
            doc.add_story(tessera_text::Story::default())
        });
        table.columns = widths;
        table.rows = heights;
        // Cells by "column:row", with their spans; the covered slots follow.
        for cell in children(node, "Cell") {
            let Some((c, r)) = attr(cell, "Name")
                .and_then(|n| n.split_once(':'))
                .and_then(|(c, r)| Some((c.parse::<usize>().ok()?, r.parse::<usize>().ok()?)))
            else {
                continue;
            };
            let span = Span {
                columns: attr_f64(cell, "ColumnSpan").map_or(1, |n| n as u16).max(1),
                rows: attr_f64(cell, "RowSpan").map_or(1, |n| n as u16).max(1),
            };
            // A cross-reference inside a table cell keeps the words InDesign
            // wrote for it rather than becoming a live reference: the cells
            // are read after the stories, without the spine's links to hand.
            let read = story::read(cell, styles, colours, &story::Links::default());
            if !read.inline.is_empty() {
                dropped.note("an object anchored inside a table cell");
            }
            let Some(slot) = table.at_mut(r, c) else {
                continue;
            };
            let Slot::Cell(existing) = slot else { continue };
            if let Some(s) = doc.story_mut(existing.story) {
                *s = read.story;
            }
            existing.span = span;
            // What the span covers is not a cell of its own.
            for rr in r..r + usize::from(span.rows) {
                for cc in c..c + usize::from(span.columns) {
                    if (rr, cc) != (r, c)
                        && let Some(covered) = table.at_mut(rr, cc)
                    {
                        *covered = Slot::Covered;
                    }
                }
            }
        }
        let height: f64 = table.rows.iter().sum();
        let stroke = self
            .colours
            .get(attr(node, "StrokeColor").or(Some("Color/Black")))
            .map(|c| Stroke::new(c, attr_f64(node, "StrokeWeight").unwrap_or(0.5)));
        table.stroke = stroke;
        let layer = self.fallback_layer;
        let id = doc.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height: height.max(1.0),
                },
                kind: FrameKind::Table(table),
                transform: Transform::IDENTITY,
                fill: Paint::Solid(Color::Rgb {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                }),
                stroke: None,
                wrap: tessera_document::nodes::TextWrap::None,
                blend: tessera_document::blending::Blending::PLAIN,
                corners: tessera_document::corners::Corners::SQUARE,
                shadow: None,
                anchor: Some(tessera_document::anchored::Anchored::new(story, index)),
                style: None,
                hidden: false,
                locked: false,
            },
        );
        if let Some(name) = attr(node, "Self") {
            self.frames.insert(name.to_owned(), id);
        }
    }

    fn place(
        &mut self,
        node: Node,
        pages: &[(SpreadPage, PageId)],
        doc: &mut Document,
        dropped: &mut Dropped,
        parent: Transform,
        anchored: Option<(StoryId, usize)>,
    ) {
        let name = node.tag_name().name();
        let kind_name = match name {
            "TextFrame" | "Rectangle" | "Oval" | "Polygon" | "GraphicLine" => name,
            "Group" => {
                // Flattened: each member is placed on its own, with the
                // group's transform folded in.
                dropped.note("a group (members were placed, ungrouped)");
                let own = item_transform(node);
                let combined = own.then(parent);
                for member in node.children().filter(|n| n.is_element()) {
                    self.place(member, pages, doc, dropped, combined, None);
                }
                return;
            }
            _ => return,
        };
        if pages.is_empty() && anchored.is_none() {
            return;
        }

        let Some(points) = path_points(node) else {
            return;
        };
        let transform = item_transform(node).then(parent);
        let local = bbox(&points);
        // Where the item's corners land in spread space, to pick its page.
        let corners: Vec<(f64, f64)> = [
            (local.x, local.y),
            (local.x + local.width, local.y),
            (local.x, local.y + local.height),
            (local.x + local.width, local.y + local.height),
        ]
        .iter()
        .map(|(x, y)| apply(transform, *x, *y))
        .collect();
        let centre = (
            corners.iter().map(|c| c.0).sum::<f64>() / 4.0,
            corners.iter().map(|c| c.1).sum::<f64>() / 4.0,
        );
        // The item's origin, in Tessera's space: the spread offset re-based
        // on the page it is on. An anchored object has no page of its own —
        // the text puts it where its marker lands — so its origin is nought
        // and only its size and turn are kept.
        let (ox, oy) = if anchored.is_some() {
            (0.0, 0.0)
        } else {
            let (page, page_id) = pages
                .iter()
                .find(|(p, _)| {
                    centre.0 >= p.rect.x
                        && centre.0 <= p.rect.x + p.rect.width
                        && centre.1 >= p.rect.y
                        && centre.1 <= p.rect.y + p.rect.height
                })
                .unwrap_or(&pages[0]);
            let Some(tessera_page) = doc.pages.get(*page_id).map(|p| p.bounds) else {
                return;
            };
            let origin = apply(transform, 0.0, 0.0);
            (
                tessera_page.x + (origin.0 - page.rect.x),
                tessera_page.y + (origin.1 - page.rect.y),
            )
        };
        let [a, b, c, d, _, _] = transform.coefficients;
        let is_upright =
            (a - 1.0).abs() < 1e-9 && b.abs() < 1e-9 && c.abs() < 1e-9 && (d - 1.0).abs() < 1e-9;
        let bounds = DocRect {
            x: ox + local.x,
            y: oy + local.y,
            width: local.width.max(0.1),
            height: local.height.max(0.1),
        };
        let frame_transform = if is_upright {
            Transform::IDENTITY
        } else {
            // The linear part, turning about the item's origin where it now
            // stands: p ↦ M·(p − o) + o.
            Transform {
                coefficients: [a, b, c, d, ox - (a * ox + c * oy), oy - (b * ox + d * oy)],
            }
        };

        let fill = self
            .colours
            .paint(
                attr(node, "FillColor"),
                attr_f64(node, "FillTint"),
                attr_f64(node, "GradientFillAngle"),
            )
            .unwrap_or(Paint::Solid(Color::Rgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
        let (blend, shadow) = styles::effects(node, self.colours);
        let style =
            attr(node, "AppliedObjectStyle").and_then(|s| self.styles.object.get(s).copied());
        let stroke = match (
            self.colours.get(attr(node, "StrokeColor")),
            attr_f64(node, "StrokeWeight"),
        ) {
            (Some(colour), Some(weight)) if weight > 0.0 => Some(Stroke::new(
                tinted(colour, attr_f64(node, "StrokeTint")),
                weight,
            )),
            _ => None,
        };

        let kind = match kind_name {
            "TextFrame" => {
                let Some(story) = attr(node, "ParentStory").and_then(|s| self.stories.get(s))
                else {
                    dropped.note("a text frame whose story was missing from the package");
                    return;
                };
                if let (Some(me), Some(next)) = (attr(node, "Self"), attr(node, "NextTextFrame"))
                    && next != "n"
                {
                    self.threads.push((me.to_owned(), next.to_owned()));
                }
                FrameKind::Text {
                    story: *story,
                    layout: text_layout(node),
                }
            }
            "Rectangle" => {
                match placed_image(node) {
                    Some((path, natural)) => {
                        if !self.image_fit_noted {
                            dropped.note("placed images were fitted proportionally; their crops were not kept");
                            self.image_fit_noted = true;
                        }
                        let natural = natural.unwrap_or((bounds.width, bounds.height));
                        let link = doc.add_link(tessera_document::links::Link::new(path, natural));
                        let inner = tessera_document::graphic::fit(
                            DocRect {
                                x: 0.0,
                                y: 0.0,
                                width: bounds.width,
                                height: bounds.height,
                            },
                            natural,
                            tessera_document::graphic::Fit::Proportionally,
                        );
                        FrameKind::Graphic {
                            placed: Some(tessera_document::graphic::Placement { link, inner }),
                        }
                    }
                    None if child(node, "Image").is_some() || child(node, "PDF").is_some() => {
                        FrameKind::Graphic { placed: None }
                    }
                    None => FrameKind::Rectangle,
                }
            }
            "Oval" => FrameKind::Ellipse,
            _ => FrameKind::Path(bez_path(
                &points,
                local,
                name == "GraphicLine"
                    || attr(
                        child(node, "Properties")
                            .and_then(|p| child(p, "PathGeometry"))
                            .and_then(|g| child(g, "GeometryPathType"))
                            .unwrap_or(node),
                        "PathOpen",
                    ) == Some("true"),
            )),
        };

        let wrap = text_wrap(node, dropped);
        let layer = attr(node, "ItemLayer")
            .and_then(|l| self.layers.get(l))
            .copied()
            .unwrap_or(self.fallback_layer);
        let id = doc.add_frame(
            layer,
            Frame {
                bounds,
                kind,
                transform: frame_transform,
                fill,
                stroke,
                wrap,
                blend,
                corners: tessera_document::corners::Corners::SQUARE,
                shadow,
                anchor: anchored
                    .map(|(story, index)| tessera_document::anchored::Anchored::new(story, index)),
                style,
                // InDesign's own Object ▸ Hide and Lock, as the item was left.
                hidden: attr(node, "Visible") == Some("false"),
                locked: attr(node, "Locked") == Some("true"),
            },
        );
        if let Some(name) = attr(node, "Self") {
            self.frames.insert(name.to_owned(), id);
        }
    }
}

pub(crate) fn tinted(colour: Color, tint: Option<f64>) -> Color {
    let Some(tint) = tint.filter(|t| *t >= 0.0 && *t < 100.0) else {
        return colour;
    };
    let t = (tint / 100.0) as f32;
    match colour {
        Color::Cmyk { c, m, y, k, a } => Color::Cmyk {
            c: c * t,
            m: m * t,
            y: y * t,
            k: k * t,
            a,
        },
        Color::Rgb { r, g, b, a } => Color::Rgb {
            r: 1.0 - (1.0 - r) * t,
            g: 1.0 - (1.0 - g) * t,
            b: 1.0 - (1.0 - b) * t,
            a,
        },
        other => other,
    }
}

fn item_transform(node: Node) -> Transform {
    attr(node, "ItemTransform")
        .map(numbers)
        .filter(|t| t.len() == 6)
        .map(|t| Transform {
            coefficients: [t[0], t[1], t[2], t[3], t[4], t[5]],
        })
        .unwrap_or(Transform::IDENTITY)
}

fn apply(t: Transform, x: f64, y: f64) -> (f64, f64) {
    let [a, b, c, d, e, f] = t.coefficients;
    (a * x + c * y + e, b * x + d * y + f)
}

/// One path point: its anchor and the two handles.
#[derive(Debug, Clone, Copy)]
struct PathPoint {
    anchor: (f64, f64),
    left: (f64, f64),
    right: (f64, f64),
}

fn path_points(node: Node) -> Option<Vec<PathPoint>> {
    let properties = child(node, "Properties")?;
    let geometry = child(properties, "PathGeometry")?;
    let path = child(geometry, "GeometryPathType")?;
    let array = child(path, "PathPointArray")?;
    let points: Vec<PathPoint> = children(array, "PathPointType")
        .filter_map(|p| {
            let pair = |name: &str| -> Option<(f64, f64)> {
                let n = attr(p, name).map(numbers)?;
                Some((*n.first()?, *n.get(1)?))
            };
            let anchor = pair("Anchor")?;
            Some(PathPoint {
                anchor,
                left: pair("LeftDirection").unwrap_or(anchor),
                right: pair("RightDirection").unwrap_or(anchor),
            })
        })
        .collect();
    if points.is_empty() {
        None
    } else {
        Some(points)
    }
}

fn bbox(points: &[PathPoint]) -> DocRect {
    let xs = points.iter().map(|p| p.anchor.0);
    let ys = points.iter().map(|p| p.anchor.1);
    let (x0, x1) = (
        xs.clone().fold(f64::INFINITY, f64::min),
        xs.fold(f64::NEG_INFINITY, f64::max),
    );
    let (y0, y1) = (
        ys.clone().fold(f64::INFINITY, f64::min),
        ys.fold(f64::NEG_INFINITY, f64::max),
    );
    DocRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// The points as a path in the frame's own space: the bounding box's corner
/// at the origin, curves kept.
fn bez_path(points: &[PathPoint], local: DocRect, open: bool) -> kurbo::BezPath {
    let mut path = kurbo::BezPath::new();
    let at = |p: (f64, f64)| kurbo::Point::new(p.0 - local.x, p.1 - local.y);
    let Some(first) = points.first() else {
        return path;
    };
    path.move_to(at(first.anchor));
    let segments: Vec<(&PathPoint, &PathPoint)> = points
        .windows(2)
        .map(|w| (&w[0], &w[1]))
        .chain(if open {
            None
        } else {
            points.last().zip(points.first())
        })
        .collect();
    for (from, to) in segments {
        if from.right == from.anchor && to.left == to.anchor {
            path.line_to(at(to.anchor));
        } else {
            path.curve_to(at(from.right), at(to.left), at(to.anchor));
        }
    }
    if !open {
        path.close_path();
    }
    path
}

/// `<TextWrapPreference TextWrapMode="…" TextWrapSide="…">` with its
/// offsets.
///
/// A side named against the spine is dropped out loud and read as the
/// largest area: which side the spine is on is a fact about the page the
/// object lands on, and the wrap is a fact about the object.
fn text_wrap(node: Node, dropped: &mut Dropped) -> tessera_document::nodes::TextWrap {
    use tessera_document::nodes::{TextWrap, WrapTo};
    let Some(pref) = child(node, "TextWrapPreference") else {
        return TextWrap::None;
    };
    let sides = match attr(pref, "TextWrapSide") {
        Some("BothSides") => WrapTo::Both,
        Some("LeftSide") => WrapTo::Left,
        Some("RightSide") => WrapTo::Right,
        Some("SideTowardsSpine") | Some("SideAwayFromSpine") => {
            dropped.note("a text wrap side named against the spine (read as the largest area)");
            WrapTo::Largest
        }
        _ => WrapTo::Largest,
    };
    let offset = child(pref, "Properties")
        .and_then(|p| child(p, "TextWrapOffset"))
        .map(|o| Insets {
            top: attr_f64(o, "Top").unwrap_or(0.0),
            left: attr_f64(o, "Left").unwrap_or(0.0),
            bottom: attr_f64(o, "Bottom").unwrap_or(0.0),
            right: attr_f64(o, "Right").unwrap_or(0.0),
        })
        .unwrap_or_default();
    match attr(pref, "TextWrapMode") {
        Some("BoundingBoxTextWrap") => TextWrap::Bounds {
            standoff: offset,
            sides,
        },
        Some("Contour") => TextWrap::Contour {
            standoff: offset.top,
            sides,
        },
        Some("JumpObjectTextWrap") | Some("NextFrameTextWrap") => TextWrap::Jump,
        _ => TextWrap::None,
    }
}

/// A text frame's columns, insets and vertical alignment.
fn text_layout(node: Node) -> TextLayout {
    let mut layout = TextLayout::default();
    let Some(pref) = child(node, "TextFramePreference") else {
        return layout;
    };
    if let Some(n) = attr_f64(pref, "TextColumnCount") {
        layout.columns = (n as u8).max(1);
    }
    if let Some(g) = attr_f64(pref, "TextColumnGutter") {
        layout.gutter = g;
    }
    layout.vertical = match attr(pref, "VerticalJustification") {
        Some("CenterAlign") => VerticalJustify::Centre,
        Some("BottomAlign") => VerticalJustify::Bottom,
        Some("JustifyAlign") => VerticalJustify::Justify,
        _ => VerticalJustify::Top,
    };
    // Either a single number on the attribute or four in the properties.
    let insets: Vec<f64> = match property(pref, "InsetSpacing") {
        Some(text) => numbers(text),
        None => child(pref, "Properties")
            .and_then(|p| child(p, "InsetSpacing"))
            .map(|list| {
                children(list, "ListItem")
                    .filter_map(|i| i.text().and_then(|t| t.trim().parse().ok()))
                    .collect()
            })
            .or_else(|| attr(pref, "InsetSpacing").map(numbers))
            .unwrap_or_default(),
    };
    layout.inset = match insets.as_slice() {
        [all] => Insets {
            top: *all,
            bottom: *all,
            left: *all,
            right: *all,
        },
        [top, left, bottom, right] => Insets {
            top: *top,
            left: *left,
            bottom: *bottom,
            right: *right,
        },
        _ => layout.inset,
    };
    layout
}

/// The image a rectangle holds, and its natural size when the package says.
fn placed_image(node: Node) -> Option<(PathBuf, Option<(f64, f64)>)> {
    let image = ["Image", "PDF", "EPS", "ImportedPage"]
        .iter()
        .find_map(|name| child(node, name))?;
    let link = child(image, "Link")?;
    let uri = attr(link, "LinkResourceURI")?;
    let path = path_from_uri(uri);
    let natural = child(image, "Properties")
        .and_then(|p| child(p, "GraphicBounds"))
        .and_then(|g| {
            Some((
                attr_f64(g, "Right")? - attr_f64(g, "Left")?,
                attr_f64(g, "Bottom")? - attr_f64(g, "Top")?,
            ))
        });
    Some((path, natural))
}

/// `file:/C:/Users/x/a.jpg` and `file:///Users/x/a.jpg` to a path.
fn path_from_uri(uri: &str) -> PathBuf {
    let rest = uri.strip_prefix("file:").unwrap_or(uri);
    let rest = rest.trim_start_matches('/');
    let decoded = percent_decode(rest);
    // A Windows drive keeps its colon; a Unix path gets its root back.
    if decoded.len() > 1 && decoded.as_bytes()[1] == b':' {
        PathBuf::from(decoded)
    } else {
        PathBuf::from(format!("/{decoded}"))
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // `get`, not a slice: a `%` followed by a multi-byte character would
        // put the range inside it, and a link path is not worth a crash.
        if bytes[i] == b'%'
            && let Some(digits) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(digits, 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            path_from_uri("file:/C:/Users/x/a%20b.jpg"),
            PathBuf::from("C:/Users/x/a b.jpg")
        );
        assert_eq!(
            path_from_uri("file:///Users/x/a.jpg"),
            PathBuf::from("/Users/x/a.jpg")
        );
    }

    #[test]
    fn a_percent_before_an_accented_letter_is_kept_rather_than_crashing() {
        // "%é": the two bytes after the percent sign are one character, and
        // slicing them as hex digits used to panic inside it.
        assert_eq!(percent_decode("caf%é"), "caf%é");
        assert_eq!(percent_decode("a%2é"), "a%2é");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn a_rotated_item_turns_about_where_it_stands() {
        // A 90° matrix with the origin at (100, 100): the transform maps
        // (100, 100) to itself and a point to its right to a point below.
        let t = Transform {
            coefficients: [0.0, 1.0, -1.0, 0.0, 0.0, 0.0],
        };
        let [a, b, c, d, _, _] = t.coefficients;
        let (ox, oy) = (100.0, 100.0);
        let re = Transform {
            coefficients: [a, b, c, d, ox - (a * ox + c * oy), oy - (b * ox + d * oy)],
        };
        assert_eq!(apply(re, 100.0, 100.0), (100.0, 100.0));
        assert_eq!(apply(re, 110.0, 100.0), (100.0, 110.0));
    }
}
