//! Building the PDF.

use std::collections::BTreeMap;

use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};
use tessera_color::Color;
use tessera_geometry::DocRect;
use tessera_layout::resolve::{ResolvedDocument, ResolvedKind};
use tessera_text::shape::{FontData, ShapedText};

/// PDF expresses glyph metrics in thousandths of an em.
const PDF_UNITS_PER_EM: f64 = 1000.0;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("could not subset {family}: {source}")]
    Subset {
        family: String,
        #[source]
        source: subsetter::Error,
    },
    #[error(
        "glyph id {0} does not fit a 16-bit CID, so it cannot be written to a PDF \
         with Identity-H encoding"
    )]
    GlyphIdTooLarge(u32),
}

/// Convert a document-space y to PDF space.
///
/// PDF's origin is bottom-left; the document's is top-left. **This is the only
/// place that conversion happens.** Scattering the flip is how an exporter
/// ends up subtly disagreeing with the screen.
fn to_pdf_y(page: DocRect, doc_y: f64, height: f64) -> f64 {
    page.height - doc_y - height
}

/// One embedded font: its subset bytes, its glyph mapping and its metrics.
struct EmbeddedFont {
    /// Subset font bytes.
    data: Vec<u8>,
    /// Original glyph id to subset glyph id.
    remap: BTreeMap<u16, u16>,
    /// Subset glyph id to advance width, in PDF units.
    widths: BTreeMap<u16, f64>,
    resource: String,
    font_ref: Ref,
    cid_ref: Ref,
    descriptor_ref: Ref,
    file_ref: Ref,
}

pub fn export(resolved: &ResolvedDocument, page: DocRect) -> Result<Vec<u8>, PdfError> {
    let mut pdf = Pdf::new();
    let mut next = 1;
    let mut alloc = || {
        let r = Ref::new(next);
        next += 1;
        r
    };

    let catalog_id = alloc();
    let page_tree_id = alloc();
    let page_id = alloc();
    let content_id = alloc();

    let fonts = collect_fonts(resolved, &mut alloc)?;
    let content = build_content(resolved, page, &fonts)?;

    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id).kids([page_id]).count(1);

    {
        let mut page_obj = pdf.page(page_id);
        page_obj
            .parent(page_tree_id)
            .media_box(Rect::new(0.0, 0.0, page.width as f32, page.height as f32))
            .contents(content_id);
        let mut resources = page_obj.resources();
        let mut font_dict = resources.fonts();
        for font in &fonts {
            font_dict.pair(Name(font.resource.as_bytes()), font.font_ref);
        }
        font_dict.finish();
        resources.finish();
        page_obj.finish();
    }

    // Uncompressed in milestone 0 so the operators are assertable and a
    // damaged file stays inspectable. Milestone 6 owns export quality and
    // turns on compression there.
    pdf.stream(content_id, &content);

    for font in &fonts {
        write_font(&mut pdf, font);
    }

    Ok(pdf.finish())
}

fn collect_fonts(
    resolved: &ResolvedDocument,
    alloc: &mut impl FnMut() -> Ref,
) -> Result<Vec<EmbeddedFont>, PdfError> {
    // Group the glyphs actually drawn, per font, so only those are embedded.
    let mut used: Vec<(FontData, Vec<u16>, BTreeMap<u16, f64>)> = Vec::new();

    for item in &resolved.items {
        let ResolvedKind::Text { shaped, .. } = &item.kind else {
            continue;
        };
        for (index, font) in shaped.fonts.iter().enumerate() {
            let slot = match used.iter().position(|(f, _, _)| f == font) {
                Some(i) => i,
                None => {
                    used.push((font.clone(), Vec::new(), BTreeMap::new()));
                    used.len() - 1
                }
            };
            for glyph in shaped
                .lines
                .iter()
                .flat_map(|l| l.glyphs.iter())
                .filter(|g| g.font_index == index)
            {
                let id = u16::try_from(glyph.glyph_id)
                    .map_err(|_| PdfError::GlyphIdTooLarge(glyph.glyph_id))?;
                used[slot].1.push(id);
                // Advance is carried from the shaper, in points at the shaped
                // size, so scaling to PDF units needs only the font size.
                let width = glyph.advance / f64::from(shaped.font_size) * PDF_UNITS_PER_EM;
                used[slot].2.insert(id, width);
            }
        }
    }

    let mut fonts = Vec::new();
    for (i, (font, glyphs, advances)) in used.into_iter().enumerate() {
        let remapper = subsetter::GlyphRemapper::new_from_glyphs(&glyphs);
        let data =
            subsetter::subset(font.data.as_ref(), font.index, &remapper).map_err(|source| {
                PdfError::Subset {
                    family: format!("font {i}"),
                    source,
                }
            })?;

        let mut remap = BTreeMap::new();
        let mut widths = BTreeMap::new();
        for old in glyphs {
            if let Some(new) = remapper.get(old) {
                remap.insert(old, new);
                if let Some(w) = advances.get(&old) {
                    widths.insert(new, *w);
                }
            }
        }

        fonts.push(EmbeddedFont {
            data,
            remap,
            widths,
            resource: format!("F{i}"),
            font_ref: alloc(),
            cid_ref: alloc(),
            descriptor_ref: alloc(),
            file_ref: alloc(),
        });
    }

    Ok(fonts)
}

fn build_content(
    resolved: &ResolvedDocument,
    page: DocRect,
    fonts: &[EmbeddedFont],
) -> Result<Vec<u8>, PdfError> {
    let mut content = Content::new();

    for item in &resolved.items {
        match &item.kind {
            ResolvedKind::Rectangle { fill, .. } => {
                let [r, g, b, _] = fill.to_rgb_f32();
                content.save_state();
                content.set_fill_rgb(r, g, b);
                content.rect(
                    item.bounds.x as f32,
                    to_pdf_y(page, item.bounds.y, item.bounds.height) as f32,
                    item.bounds.width as f32,
                    item.bounds.height as f32,
                );
                content.fill_nonzero();
                content.restore_state();
            }

            ResolvedKind::Ellipse { fill, .. } => {
                let [r, g, b, _] = fill.to_rgb_f32();
                content.save_state();
                content.set_fill_rgb(r, g, b);
                ellipse_path(&mut content, page, item.bounds);
                content.fill_nonzero();
                content.restore_state();
            }

            ResolvedKind::Path { path, fill, stroke } => {
                content.save_state();
                emit_path(&mut content, page, item.bounds, path);
                match (fill, stroke) {
                    (Some(f), _) => {
                        let [r, g, b, _] = f.to_rgb_f32();
                        content.set_fill_rgb(r, g, b);
                        content.fill_nonzero();
                    }
                    (None, Some(s)) => {
                        let [r, g, b, _] = s.color.to_rgb_f32();
                        content.set_stroke_rgb(r, g, b);
                        content.set_line_width(s.width as f32);
                        content.stroke();
                    }
                    (None, None) => {
                        // Nothing to paint; the path was still emitted, so
                        // end it rather than leaving a dangling path object.
                        content.end_path();
                    }
                }
                content.restore_state();
            }

            ResolvedKind::Text { shaped, color } => {
                draw_text(&mut content, page, item.bounds, shaped, color, fonts)?;
            }
        }
    }

    Ok(content.finish().to_vec())
}

/// Emit a frame-local path into the content stream, in PDF coordinates.
///
/// Quadratics are raised to cubics because PDF has no quadratic operator.
fn emit_path(content: &mut Content, page: DocRect, bounds: DocRect, path: &kurbo::BezPath) {
    let at = |p: kurbo::Point| {
        (
            (bounds.x + p.x) as f32,
            to_pdf_y(page, bounds.y + p.y, 0.0) as f32,
        )
    };

    let mut current = kurbo::Point::ZERO;
    for el in path.elements() {
        match *el {
            kurbo::PathEl::MoveTo(p) => {
                let (x, y) = at(p);
                content.move_to(x, y);
                current = p;
            }
            kurbo::PathEl::LineTo(p) => {
                let (x, y) = at(p);
                content.line_to(x, y);
                current = p;
            }
            kurbo::PathEl::QuadTo(c, p) => {
                // Degree elevation: a quadratic (P0, C, P1) is the cubic
                // (P0, P0 + 2/3(C - P0), P1 + 2/3(C - P1), P1).
                let c1 = current + (c - current) * (2.0 / 3.0);
                let c2 = p + (c - p) * (2.0 / 3.0);
                let (x1, y1) = at(c1);
                let (x2, y2) = at(c2);
                let (x, y) = at(p);
                content.cubic_to(x1, y1, x2, y2, x, y);
                current = p;
            }
            kurbo::PathEl::CurveTo(c1, c2, p) => {
                let (x1, y1) = at(c1);
                let (x2, y2) = at(c2);
                let (x, y) = at(p);
                content.cubic_to(x1, y1, x2, y2, x, y);
                current = p;
            }
            kurbo::PathEl::ClosePath => {
                content.close_path();
            }
        }
    }
}

/// Four cubic segments, the standard circle approximation.
fn ellipse_path(content: &mut Content, page: DocRect, b: DocRect) {
    const K: f64 = 0.552_284_749_8;
    let (rx, ry) = (b.width / 2.0, b.height / 2.0);
    let cx = b.x + rx;
    let cy = to_pdf_y(page, b.y, b.height) + ry;
    let (ox, oy) = (rx * K, ry * K);

    content.move_to((cx - rx) as f32, cy as f32);
    content.cubic_to(
        (cx - rx) as f32,
        (cy + oy) as f32,
        (cx - ox) as f32,
        (cy + ry) as f32,
        cx as f32,
        (cy + ry) as f32,
    );
    content.cubic_to(
        (cx + ox) as f32,
        (cy + ry) as f32,
        (cx + rx) as f32,
        (cy + oy) as f32,
        (cx + rx) as f32,
        cy as f32,
    );
    content.cubic_to(
        (cx + rx) as f32,
        (cy - oy) as f32,
        (cx + ox) as f32,
        (cy - ry) as f32,
        cx as f32,
        (cy - ry) as f32,
    );
    content.cubic_to(
        (cx - ox) as f32,
        (cy - ry) as f32,
        (cx - rx) as f32,
        (cy - oy) as f32,
        (cx - rx) as f32,
        cy as f32,
    );
    content.close_path();
}

fn draw_text(
    content: &mut Content,
    page: DocRect,
    bounds: DocRect,
    shaped: &ShapedText,
    color: &Color,
    fonts: &[EmbeddedFont],
) -> Result<(), PdfError> {
    let [r, g, b, _] = color.to_rgb_f32();

    for (index, font_data) in shaped.fonts.iter().enumerate() {
        // Match by subset content: `collect_fonts` walked the same items in
        // the same order, so position `index` here maps to the same font.
        let Some(embedded) = fonts.get(font_for(shaped, index, fonts)) else {
            continue;
        };
        let _ = font_data;

        content.save_state();
        content.set_fill_rgb(r, g, b);
        content.begin_text();
        content.set_font(Name(embedded.resource.as_bytes()), shaped.font_size);

        for glyph in shaped
            .lines
            .iter()
            .flat_map(|l| l.glyphs.iter())
            .filter(|glyph| glyph.font_index == index)
        {
            let old = u16::try_from(glyph.glyph_id)
                .map_err(|_| PdfError::GlyphIdTooLarge(glyph.glyph_id))?;
            let Some(cid) = embedded.remap.get(&old) else {
                continue;
            };

            // Positions come straight from the shaper. Recomputing them here
            // is exactly how an export drifts away from the screen.
            let x = bounds.x + glyph.x;
            let y = to_pdf_y(page, bounds.y + glyph.y, 0.0);
            content.next_line(0.0, 0.0);
            content.set_text_matrix([1.0, 0.0, 0.0, 1.0, x as f32, y as f32]);
            content.show(Str(&cid.to_be_bytes()));
        }

        content.end_text();
        content.restore_state();
    }

    Ok(())
}

/// Milestone 0 embeds one font, so index and slot coincide. Kept as a named
/// function so milestone 2's font cache has an obvious place to change.
fn font_for(_shaped: &ShapedText, index: usize, fonts: &[EmbeddedFont]) -> usize {
    index.min(fonts.len().saturating_sub(1))
}

fn write_font(pdf: &mut Pdf, font: &EmbeddedFont) {
    let base = format!("Tessera+F{}", font.font_ref.get());

    // Identity-H lets glyph ids be written directly, which is precisely what a
    // shaper produces — no character-code round trip in between.
    pdf.type0_font(font.font_ref)
        .base_font(Name(base.as_bytes()))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(font.cid_ref)
        .finish();

    let mut cid = pdf.cid_font(font.cid_ref);
    cid.subtype(pdf_writer::types::CidFontType::Type2)
        .base_font(Name(base.as_bytes()))
        .system_info(pdf_writer::types::SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        })
        .font_descriptor(font.descriptor_ref)
        .default_width(PDF_UNITS_PER_EM as f32);
    {
        let mut widths = cid.widths();
        for (gid, w) in &font.widths {
            widths.consecutive(*gid, [*w as f32]);
        }
        widths.finish();
    }
    cid.cid_to_gid_map_predefined(Name(b"Identity"));
    cid.finish();

    pdf.font_descriptor(font.descriptor_ref)
        .name(Name(base.as_bytes()))
        .flags(pdf_writer::types::FontFlags::SYMBOLIC)
        .bbox(Rect::new(-1000.0, -1000.0, 2000.0, 2000.0))
        .italic_angle(0.0)
        .ascent(800.0)
        .descent(-200.0)
        .cap_height(700.0)
        .stem_v(80.0)
        .font_file2(font.file_ref)
        .finish();

    pdf.stream(font.file_ref, &font.data).finish();
}
