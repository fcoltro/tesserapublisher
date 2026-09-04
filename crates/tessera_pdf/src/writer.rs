//! Building the PDF.

use std::collections::BTreeMap;

use pdf_writer::types::{LineCapStyle, LineJoinStyle};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};
use tessera_color::Color;
use tessera_document::nodes::{LineCap, LineJoin, Stroke};
use tessera_geometry::{DocRect, Transform};
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
/// The page a document with no pages exports as. Unreachable through the
/// application, which always has one; a default beats a panic in a writer.
const DEFAULT_PAGE: tessera_layout::ResolvedPage = tessera_layout::ResolvedPage {
    bounds: LETTER,
    margins: LETTER,
    bleed: LETTER,
    slug: LETTER,
};

const LETTER: DocRect = DocRect {
    x: 0.0,
    y: 0.0,
    width: 612.0,
    height: 792.0,
};

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

pub fn export(resolved: &ResolvedDocument) -> Result<Vec<u8>, PdfError> {
    // The page comes from the resolved document rather than from a parameter,
    // so the screen and the PDF cannot disagree about where the trim is.
    // Milestone 3 makes this every page; today it is the first.
    let resolved_page = resolved.pages.first().copied().unwrap_or(DEFAULT_PAGE);
    let page = resolved_page.bounds;
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
        // MediaBox must contain everything imaged, so it is the bleed when
        // there is one. TrimBox is the finished page — where the guillotine
        // goes — and BleedBox is how far the ink runs past it. A printer reads
        // those two, not MediaBox, so exporting a document with a bleed and
        // recording only one rectangle discards the user's intent silently.
        //
        // The origin stays at the trim corner, so adding a bleed does not
        // move a single object on the page; the boxes grow around the content
        // rather than shifting it.
        let bleed = resolved_page.bleed;
        let media = Rect::new(
            (bleed.x - page.x) as f32,
            (bleed.y - page.y) as f32,
            (bleed.x - page.x + bleed.width) as f32,
            (bleed.y - page.y + bleed.height) as f32,
        );
        let trim = Rect::new(0.0, 0.0, page.width as f32, page.height as f32);

        page_obj
            .parent(page_tree_id)
            .media_box(media)
            .trim_box(trim)
            .bleed_box(media)
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
        // A placed item gets its own graphics state, with its transform
        // written as a `cm` matrix.
        let placed = !item.transform.is_identity();
        if placed {
            content.save_state();
            content.transform(to_pdf_matrix(item.transform, page).map(|v| v as f32));
        }

        match &item.kind {
            ResolvedKind::Rectangle { fill, stroke } => {
                content.save_state();
                let [r, g, b, _] = fill.to_rgb_f32();
                content.set_fill_rgb(r, g, b);
                let rect = |c: &mut Content, b: DocRect| {
                    c.rect(
                        b.x as f32,
                        to_pdf_y(page, b.y, b.height) as f32,
                        b.width as f32,
                        b.height as f32,
                    );
                };

                match stroke {
                    // The fill and the stroke follow different rectangles once
                    // the stroke is aligned inside or outside, so they cannot
                    // share one path.
                    Some(s) => {
                        rect(&mut content, item.bounds);
                        content.fill_nonzero();
                        apply_stroke(&mut content, s);
                        rect(&mut content, offset_rect(item.bounds, s.offset()));
                        content.stroke();
                    }
                    None => {
                        rect(&mut content, item.bounds);
                        content.fill_nonzero();
                    }
                }
                content.restore_state();
            }

            ResolvedKind::Ellipse { fill, stroke } => {
                content.save_state();
                let [r, g, b, _] = fill.to_rgb_f32();
                content.set_fill_rgb(r, g, b);
                ellipse_path(&mut content, page, item.bounds);
                content.fill_nonzero();
                if let Some(s) = stroke {
                    apply_stroke(&mut content, s);
                    ellipse_path(&mut content, page, offset_rect(item.bounds, s.offset()));
                    content.stroke();
                }
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
                        apply_stroke(&mut content, s);
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

        if placed {
            content.restore_state();
        }
    }

    Ok(content.finish().to_vec())
}

/// Set every stroke attribute on the content stream.
///
/// Colour, width, cap, join, miter limit and dash pattern. A stroke that
/// exported as a bare width would not be the stroke that was on screen, which
/// is the one thing this crate exists to prevent.
fn apply_stroke(content: &mut Content, stroke: &Stroke) {
    let [r, g, b, _] = stroke.color.to_rgb_f32();
    content.set_stroke_rgb(r, g, b);
    content.set_line_width(stroke.width as f32);
    content.set_line_cap(match stroke.cap {
        LineCap::Butt => LineCapStyle::ButtCap,
        LineCap::Round => LineCapStyle::RoundCap,
        LineCap::Square => LineCapStyle::ProjectingSquareCap,
    });
    content.set_line_join(match stroke.join {
        LineJoin::Miter => LineJoinStyle::MiterJoin,
        LineJoin::Round => LineJoinStyle::RoundJoin,
        LineJoin::Bevel => LineJoinStyle::BevelJoin,
    });
    content.set_miter_limit(stroke.miter_limit as f32);
    if stroke.is_dashed() {
        content.set_dash_pattern(
            stroke.dashes.iter().map(|d| *d as f32),
            stroke.dash_offset as f32,
        );
    }
}

/// A rectangle moved out to where an aligned stroke's centreline runs.
///
/// Held at the point where an inside stroke would turn the rectangle inside
/// out, exactly as the screen renderer holds it.
fn offset_rect(bounds: DocRect, offset: f64) -> DocRect {
    let limit = (bounds.width.min(bounds.height) / 2.0).max(0.0);
    let o = offset.max(-limit);
    DocRect {
        x: bounds.x - o,
        y: bounds.y - o,
        width: bounds.width + o * 2.0,
        height: bounds.height + o * 2.0,
    }
}

/// A document-space transform, expressed in PDF's coordinate space.
///
/// The rest of this writer converts each coordinate as it emits it, through
/// [`to_pdf_y`], so the content stream is already in PDF space — where y grows
/// upward rather than downward. A transform written for document space
/// therefore has to be mirrored into that space before it can be applied to
/// it: `F * A * F`, where `F` is the y flip. `F` is its own inverse, which is
/// why it appears on both sides.
///
/// For a pure rotation this comes out as the same negated angle the writer
/// used to compute by hand — now a consequence of the mirroring rather than a
/// separate rule to keep in step.
fn to_pdf_matrix(transform: Transform, page: DocRect) -> [f64; 6] {
    let flip = kurbo::Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, page.height]);
    (flip * transform.to_affine() * flip).as_coeffs()
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

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocPoint;

    fn page() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 595.0,
            height: 842.0,
        }
    }

    /// A document point, in PDF coordinates.
    fn to_pdf(page: DocRect, p: DocPoint) -> DocPoint {
        DocPoint {
            x: p.x,
            y: to_pdf_y(page, p.y, 0.0),
        }
    }

    /// The property that makes `to_pdf_matrix` right, for one transform.
    ///
    /// Placing a point and then converting it to PDF space must land in the
    /// same place as converting first and then applying the emitted matrix.
    /// If it did not, an export would disagree with the screen — which is the
    /// one thing this crate exists to prevent.
    fn agrees(transform: Transform) {
        let m = Transform {
            coefficients: to_pdf_matrix(transform, page()),
        };
        for p in [
            DocPoint { x: 0.0, y: 0.0 },
            DocPoint { x: 100.0, y: 50.0 },
            DocPoint { x: 300.0, y: 700.0 },
            DocPoint { x: -20.0, y: 900.0 },
        ] {
            let placed_then_converted = to_pdf(page(), transform.apply(p));
            let converted_then_placed = m.apply(to_pdf(page(), p));
            assert!(
                (placed_then_converted.x - converted_then_placed.x).abs() < 1e-9
                    && (placed_then_converted.y - converted_then_placed.y).abs() < 1e-9,
                "{placed_then_converted:?} vs {converted_then_placed:?}"
            );
        }
    }

    #[test]
    fn the_identity_stays_the_identity() {
        agrees(Transform::IDENTITY);
    }

    #[test]
    fn a_rotation_survives_the_mirroring() {
        for degrees in [15.0, 90.0, 180.0, -45.0] {
            agrees(Transform::rotate_about(
                degrees,
                DocPoint { x: 200.0, y: 400.0 },
            ));
        }
    }

    #[test]
    fn a_translation_moves_the_right_way_up() {
        // The direction most likely to be wrong: PDF's y grows upward, so a
        // downward move in the document is an upward one here.
        agrees(Transform::translate(10.0, 25.0));

        let m = Transform {
            coefficients: to_pdf_matrix(Transform::translate(0.0, 25.0), page()),
        };
        assert!(
            m.apply(DocPoint::ZERO).y < 0.0,
            "moving down the page must move down in PDF space too"
        );
    }

    #[test]
    fn a_scale_and_a_shear_survive_it_too() {
        agrees(Transform::scale_about(
            2.0,
            3.0,
            DocPoint { x: 50.0, y: 60.0 },
        ));
        agrees(
            Transform::rotate_about(45.0, DocPoint { x: 100.0, y: 100.0 })
                .then(Transform::scale_about(2.0, 1.0, DocPoint::ZERO)),
        );
    }

    #[test]
    fn a_rotation_comes_out_negated_as_it_did_before() {
        // The old writer computed this by hand with a negated angle. That is
        // now a consequence of the mirroring rather than a separate rule, so
        // it is worth pinning that the consequence still holds.
        let m = Transform {
            coefficients: to_pdf_matrix(
                Transform::rotate_about(30.0, DocPoint { x: 0.0, y: 0.0 }),
                page(),
            ),
        };
        assert!(
            (m.rotation_degrees() + 30.0).abs() < 1e-9,
            "got {}",
            m.rotation_degrees()
        );
    }
}
