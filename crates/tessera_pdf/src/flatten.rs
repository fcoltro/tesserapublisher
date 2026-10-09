//! Transparency flattened, for a standard that forbids it (PDF/X-1a).
//!
//! **Each area transparency touches becomes one opaque picture.** The painted
//! boxes of the transparent objects are merged where they overlap. Each area is
//! rendered with everything painted at or below its topmost transparent
//! object — the page as it composites there, through the same writer and
//! renderer as a page exported as a picture — and the picture is drawn where
//! that object was drawn. The transparent objects are then left out. What lies
//! beneath stays vector, covered inside the area by the picture, which paints
//! all of it; what lies above stays vector and untouched. Nothing is clipped,
//! so nothing has a seam to show.
//!
//! What this gives up, as InDesign's flattener does at a single resolution:
//! inside a flattened area, text and line art are pixels at the resolution
//! asked for, and an overprint there is whatever it composited as.

use std::path::PathBuf;

use tessera_geometry::{DocRect, Transform};
use tessera_layout::ResolvedPage;
use tessera_layout::resolve::{ResolvedDocument, ResolvedItem, ResolvedKind};

use crate::PdfError;
use crate::raster::{self, Colour, Format, ImageOptions};

/// A flattened document, and the pictures it places, which are deleted when
/// it is dropped.
pub(crate) struct Flattened {
    pub doc: ResolvedDocument,
    _pictures: Vec<Scratch>,
}

/// A file in the temporary directory, removed when it goes.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn union(a: DocRect, b: DocRect) -> DocRect {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = (
        (a.x + a.width).max(b.x + b.width),
        (a.y + a.height).max(b.y + b.height),
    );
    DocRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

fn overlaps(a: DocRect, b: DocRect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// An area to flatten: where it is, and the topmost transparent object in it.
struct Area {
    rect: DocRect,
    top: usize,
}

/// The areas transparency touches, merged until none overlaps another.
fn areas(resolved: &ResolvedDocument, transparent: &[bool]) -> Vec<Area> {
    // A point's margin, so the anti-aliased edge of a soft shadow is inside.
    let mut areas: Vec<Area> = resolved
        .items
        .iter()
        .enumerate()
        .filter(|(i, _)| transparent[*i])
        .map(|(i, item)| {
            let r = raster::painted(item);
            Area {
                rect: DocRect {
                    x: r.x - 1.0,
                    y: r.y - 1.0,
                    width: r.width + 2.0,
                    height: r.height + 2.0,
                },
                top: i,
            }
        })
        .filter(|a| a.rect.width > 0.0 && a.rect.height > 0.0)
        .collect();
    loop {
        let mut merged = false;
        'outer: for i in 0..areas.len() {
            for j in i + 1..areas.len() {
                if overlaps(areas[i].rect, areas[j].rect) {
                    let b = areas.remove(j);
                    areas[i].rect = union(areas[i].rect, b.rect);
                    areas[i].top = areas[i].top.max(b.top);
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            return areas;
        }
    }
}

/// `resolved` with its transparency flattened into pictures at `ppi`.
pub(crate) fn flatten(
    resolved: &ResolvedDocument,
    ppi: f64,
    progress: &crate::Progress,
) -> Result<Flattened, PdfError> {
    let transparent: Vec<bool> = resolved
        .items
        .iter()
        .map(crate::writer::item_uses_transparency)
        .collect();
    let areas = areas(resolved, &transparent);

    let options = ImageOptions {
        format: Format::Png,
        ppi,
        colour: Colour::Rgb,
        transparent: false,
        embed_profile: false,
        bleed: true,
        ..ImageOptions::default()
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut pictures = Vec::with_capacity(areas.len());
    // Each area's picture, as an item, by the index it is drawn after.
    let mut placed: Vec<(usize, ResolvedItem)> = Vec::with_capacity(areas.len());
    for (n, area) in areas.iter().enumerate() {
        progress.go_on()?;
        let items: Vec<ResolvedItem> = resolved.items[..=area.top]
            .iter()
            .filter(|item| overlaps(raster::painted(item), area.rect))
            .cloned()
            .collect();
        let page = ResolvedPage {
            bounds: area.rect,
            margins: area.rect,
            bleed: area.rect,
            slug: area.rect,
            columns: Vec::new(),
        };
        let alone = ResolvedDocument {
            items,
            pages: vec![page],
            bookmarks: Vec::new(),
        };
        let image = raster::page_images(&alone, &options, None)?
            .into_iter()
            .next()
            .ok_or_else(|| PdfError::Encode("a flattened area did not render".to_string()))?;
        let path = std::env::temp_dir().join(format!(
            "tessera-flatten-{}-{stamp}-{n}.png",
            std::process::id()
        ));
        std::fs::write(&path, &image.bytes)
            .map_err(|e| PdfError::Encode(format!("a flattened area could not be kept: {e}")))?;
        let top = &resolved.items[area.top];
        placed.push((
            area.top,
            ResolvedItem {
                frame: top.frame,
                on: top.on,
                links: Vec::new(),
                transform: Transform::IDENTITY,
                spread_area: None,
                blend: tessera_document::blending::Blending::PLAIN,
                overprint: Default::default(),
                shadow: None,
                feather: None,
                bounds: area.rect,
                kind: ResolvedKind::Graphic {
                    inner: Transform::IDENTITY,
                    source: Some(path.clone()),
                    pdf: Default::default(),
                    natural: (area.rect.width, area.rect.height),
                    missing: false,
                    stroke: None,
                },
            },
        ));
        pictures.push(Scratch(path));
    }

    let mut items = Vec::with_capacity(resolved.items.len() + placed.len());
    for (i, item) in resolved.items.iter().enumerate() {
        if !transparent[i] {
            items.push(item.clone());
        }
        // Links stay live: a link on a flattened object is kept on the item
        // that drew it, as a link with nothing to paint.
        if transparent[i] && !item.links.is_empty() {
            items.push(ResolvedItem {
                kind: ResolvedKind::Rectangle {
                    outline: None,
                    fill: tessera_document::paint::Paint::Solid(tessera_color::Color::Rgb {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }),
                    stroke: None,
                },
                shadow: None,
                feather: None,
                blend: tessera_document::blending::Blending::PLAIN,
                ..item.clone()
            });
        }
        for (_, picture) in placed.iter().filter(|(top, _)| *top == i) {
            items.push(picture.clone());
        }
    }
    Ok(Flattened {
        doc: ResolvedDocument {
            items,
            pages: resolved.pages.clone(),
            bookmarks: resolved.bookmarks.clone(),
        },
        _pictures: pictures,
    })
}
