//! Building the PDF.

use std::collections::BTreeMap;

use pdf_writer::Filter;
use pdf_writer::types::{BlendMode, LineCapStyle, LineJoinStyle};
use pdf_writer::writers::ExtGraphicsState;
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str, TextStr};
use tessera_color::Color;
use tessera_document::nodes::{LineCap, LineJoin, Stroke};
use tessera_geometry::{DocRect, Transform};
use tessera_layout::resolve::{ResolvedDocument, ResolvedKind};

use crate::ink::Ink;
use crate::options::{ExportOptions, Standard};
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
    #[error("this export cannot claim what it was asked to: {}", .0.join("; "))]
    CannotConform(Vec<String>),
    #[error("could not read {0}: {1}")]
    Unreadable(std::path::PathBuf, String),
}

impl From<std::io::Error> for PdfError {
    fn from(error: std::io::Error) -> Self {
        // The path is not known here; the caller that had one wraps it with a
        // better message. This is the fallback so `?` works on a read.
        PdfError::Unreadable(std::path::PathBuf::new(), error.to_string())
    }
}

/// Convert a document-space y to PDF space.
///
/// PDF's origin is bottom-left; the document's is top-left. **This is the only
/// place that conversion happens.** Scattering the flip is how an exporter
/// ends up subtly disagreeing with the screen.
/// The page a document with no pages exports as. Unreachable through the
/// application, which always has one; a default beats a panic in a writer.
/// The page a document with no pages exports as.
///
/// A function rather than a `const`: `ResolvedPage` carries the column guides
/// now, and a `Vec` cannot live in one. Column guides are furniture and never
/// export, so the fallback simply has none.
fn default_page() -> tessera_layout::ResolvedPage {
    tessera_layout::ResolvedPage {
        bounds: LETTER,
        margins: LETTER,
        bleed: LETTER,
        slug: LETTER,
        columns: Vec::new(),
    }
}

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

/// A plain PDF, claiming nothing.
///
/// What milestone 0 wrote, and still the right answer for a document that names
/// no press.
pub fn export(resolved: &ResolvedDocument) -> Result<Vec<u8>, PdfError> {
    export_with(resolved, &ExportOptions::default())
}

/// A PDF for a particular press, standard and set of marks.
///
/// **Refuses rather than lies.** A file claiming PDF/X it does not meet is worse
/// than one claiming nothing: a printer’s preflight believes the claim, passes
/// the file, and the job fails on press instead of in the studio.
pub fn export_with(
    resolved: &ResolvedDocument,
    options: &ExportOptions,
) -> Result<Vec<u8>, PdfError> {
    let refused = options.refusals(uses_transparency(resolved), has_artwork(resolved));
    if !refused.is_empty() {
        return Err(PdfError::CannotConform(refused));
    }
    write(resolved, options)
}

/// Whether anything in the document needs a transparency model to reproduce.
///
/// Asked before an X-1a claim is allowed, because X-1a forbids it and Tessera
/// does not flatten.
/// Whether any placed artwork will actually be written.
///
/// A frame with no file, or one whose file has gone, embeds nothing and so puts
/// no RGB in the PDF. Refusing an X-1a export because of an empty picture box
/// would be refusing over something that is not there.
fn has_artwork(resolved: &ResolvedDocument) -> bool {
    resolved.items.iter().any(|item| {
        matches!(
            &item.kind,
            ResolvedKind::Graphic {
                source: Some(_),
                ..
            }
        )
    })
}

fn uses_transparency(resolved: &ResolvedDocument) -> bool {
    resolved
        .items
        .iter()
        .any(|item| !item.blend.is_plain() || item.shadow.is_some())
}

fn write(resolved: &ResolvedDocument, options: &ExportOptions) -> Result<Vec<u8>, PdfError> {
    let ink = Ink::for_intent(options.intent.as_ref());
    // The page comes from the resolved document rather than from a parameter,
    // so the screen and the PDF cannot disagree about where the trim is.
    // Milestone 3 makes this every page; today it is the first.
    let resolved_page = resolved.pages.first().cloned().unwrap_or_else(default_page);
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
    let states = collect_states(resolved, &mut alloc);
    let shadings = collect_shadings(resolved, page, &mut alloc, &ink);
    let pictures = collect_pictures(resolved, &mut alloc, &ink);
    let shadows = collect_shadows(resolved, &mut alloc);
    let plates = collect_plates(resolved, &ink, &mut alloc);
    let content = build_content(
        resolved,
        &Written {
            page,
            fonts: &fonts,
            states: &states,
            shadings: &shadings,
            pictures: &pictures,
            shadows: &shadows,
            plates: &plates,
            ink: &ink,
            resolved_page: &resolved_page,
            options,
        },
    )?;

    // The profile is an indirect stream; the intent dictionary that points at it
    // is written inline in the catalogue, which is where PDF/X expects it.
    let profile_id = options
        .intent
        .as_ref()
        .filter(|_| options.standard != Standard::Plain)
        .map(|_| alloc());

    let mut catalog = pdf.catalog(catalog_id);
    catalog.pages(page_tree_id);
    if let (Some(profile), Some(intent)) = (profile_id, options.intent.as_ref()) {
        // **The output intent is what makes a PDF/X a PDF/X.** Without it the
        // file says which numbers to print and not what they mean, which is the
        // whole problem the standard exists to solve.
        let mut intents = catalog.output_intents();
        let mut written = intents.push();
        written
            .subtype(pdf_writer::types::OutputIntentSubtype::PDFX)
            .output_condition_identifier(TextStr(&intent.description))
            .output_condition(TextStr(&intent.description))
            .dest_output_profile(profile);
        written.finish();
        intents.finish();
    }
    catalog.finish();
    pdf.pages(page_tree_id).kids([page_id]).count(1);

    // **The claim itself.** `GTS_PDFXVersion` is what a printer’s preflight reads
    // to decide the file conforms, which is exactly why it is written only after
    // `refusals` came back empty. A file carrying this key that does not conform
    // fails on press rather than in the studio.
    if let Some(version) = options.standard.version_key() {
        let info_id = alloc();
        let mut info = pdf.document_info(info_id);
        info.pair(Name(b"GTS_PDFXVersion"), Str(version.as_bytes()));
        info.title(TextStr("Tessera document"));
        info.producer(TextStr("Tessera Publisher"));
        // A trapped state is required by PDF/X and there is no honest answer but
        // "unknown": Tessera does not trap, and claiming False would say the
        // file has been checked and needs none.
        info.trapped(pdf_writer::types::TrappingStatus::Unknown);
        info.finish();
    }

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
        // MediaBox has to hold everything imaged, and marks are imaged outside
        // the bleed. A media box that stopped at the bleed would crop the crop
        // marks, which is the kind of failure that is only noticed on the
        // proof.
        let reach = options.marks.reach();
        let bleed = resolved_page.bleed;
        let media = Rect::new(
            (bleed.x - page.x - reach) as f32,
            (bleed.y - page.y - reach) as f32,
            (bleed.x - page.x + bleed.width + reach) as f32,
            (bleed.y - page.y + bleed.height + reach) as f32,
        );
        let trim = Rect::new(0.0, 0.0, page.width as f32, page.height as f32);

        // BleedBox is the bleed, not the media box. They were the same while
        // there were no marks; with marks the media box is larger, and saying
        // the ink runs that far would be a lie a printer acts on.
        let bleed_box = Rect::new(
            (bleed.x - page.x) as f32,
            (bleed.y - page.y) as f32,
            (bleed.x - page.x + bleed.width) as f32,
            (bleed.y - page.y + bleed.height) as f32,
        );
        page_obj
            .parent(page_tree_id)
            .media_box(media)
            .trim_box(trim)
            .bleed_box(bleed_box)
            .contents(content_id);
        let mut resources = page_obj.resources();
        let mut font_dict = resources.fonts();
        for font in &fonts {
            font_dict.pair(Name(font.resource.as_bytes()), font.font_ref);
        }
        font_dict.finish();
        if !states.is_empty() {
            let mut state_dict = resources.ext_g_states();
            for state in &states {
                state_dict.pair(Name(state.resource.as_bytes()), state.id);
            }
            state_dict.finish();
        }
        let used: Vec<&Shading> = shadings.iter().flatten().collect();
        if !used.is_empty() {
            let mut shading_dict = resources.shadings();
            for shading in &used {
                shading_dict.pair(Name(shading.resource.as_bytes()), shading.id);
            }
            shading_dict.finish();
        }
        let cast: Vec<&CastShadow> = shadows.iter().flatten().collect();
        if !pictures.is_empty() || !cast.is_empty() {
            let mut objects = resources.x_objects();
            for picture in &pictures {
                objects.pair(Name(picture.resource.as_bytes()), picture.id);
            }
            for shadow in &cast {
                objects.pair(Name(shadow.resource.as_bytes()), shadow.id);
            }
            objects.finish();
        }
        if !plates.is_empty() {
            let mut spaces = resources.color_spaces();
            for plate in &plates {
                spaces.pair(Name(plate.resource.as_bytes()), plate.id);
            }
            spaces.finish();
        }
        resources.finish();
        page_obj.finish();
    }

    // Uncompressed in milestone 0 so the operators are assertable and a
    // damaged file stays inspectable. Milestone 6 owns export quality and
    // turns on compression there.
    pdf.stream(content_id, &content);
    write_pictures(&mut pdf, &pictures);
    write_shadows(&mut pdf, &shadows);
    write_plates(&mut pdf, &plates);

    for font in &fonts {
        write_font(&mut pdf, font);
    }
    for state in &states {
        write_state(&mut pdf, state);
    }
    for shading in shadings.iter().flatten() {
        write_shading(&mut pdf, shading, &ink);
    }

    if let (Some(profile), Some(intent)) = (profile_id, options.intent.as_ref()) {
        // The profile itself, embedded. A file that names a press without
        // carrying its profile is a claim nobody downstream can check, and
        // PDF/X requires it for exactly that reason.
        let mut stream = pdf.stream(profile, &intent.profile);
        stream.pair(Name(b"N"), if ink.is_cmyk() { 4 } else { 3 });
        stream.finish();
    }

    Ok(pdf.finish())
}

/// One `/ExtGState` the page will refer to by name.
struct GraphicsState {
    resource: String,
    id: Ref,
    blend: tessera_document::blending::Blending,
}

/// The distinct compositing states the document uses.
///
/// Deduplicated, because a PDF names graphics states in a page resource
/// dictionary and forty objects at 50% should be one entry rather than forty.
/// Keyed on the opacity's *bits* rather than on the value, since two f32s that
/// compare equal are the same state and NaN is not a state at all.
///
/// **A known shortfall, stated rather than hidden.** `/ca` and `/CA` are
/// per-paint alphas, so an object with both a fill and a stroke has each of
/// them made translucent separately here, and its stroke shows faintly through
/// its own fill — where the screen composites the object as one group and it
/// does not. Closing that needs a transparency-group form XObject per object,
/// which belongs with the rest of export quality in milestone 6.
fn collect_states(
    resolved: &ResolvedDocument,
    alloc: &mut impl FnMut() -> Ref,
) -> Vec<GraphicsState> {
    let mut seen: BTreeMap<(u8, u32), GraphicsState> = BTreeMap::new();

    for item in &resolved.items {
        // A plain object is painted straight onto the page and needs no state.
        // An invisible one is not written at all, exactly as it is not drawn.
        if item.blend.is_plain() || item.blend.is_invisible() {
            continue;
        }
        let key = (item.blend.mode as u8, item.blend.alpha().to_bits());
        let next = seen.len();
        seen.entry(key).or_insert_with(|| GraphicsState {
            resource: format!("GS{next}"),
            id: alloc(),
            blend: item.blend,
        });
    }

    seen.into_values().collect()
}

fn write_state(pdf: &mut Pdf, state: &GraphicsState) {
    let alpha = state.blend.alpha();
    let mut written = pdf.indirect(state.id).start::<ExtGraphicsState>();
    written
        .non_stroking_alpha(alpha)
        .stroking_alpha(alpha)
        .blend_mode(to_pdf_blend(state.blend.mode));
    written.finish();
}

/// The document's blend mode as PDF names it.
///
/// Only the separable modes the model offers, and deliberately no catch-all
/// arm: a mode added to the model must be answered for here rather than
/// quietly exporting as Normal.
fn to_pdf_blend(mode: tessera_document::blending::BlendMode) -> BlendMode {
    use tessera_document::blending::BlendMode as Ours;
    match mode {
        Ours::Normal => BlendMode::Normal,
        Ours::Multiply => BlendMode::Multiply,
        Ours::Screen => BlendMode::Screen,
        Ours::Overlay => BlendMode::Overlay,
    }
}

/// One `/Shading` the page will refer to by name, and the function objects it
/// needs.
struct Shading {
    resource: String,
    id: Ref,
    /// The stops, as the exponential functions a PDF ramp is made of, plus the
    /// stitching function that joins them. One `Ref` per interval and one for
    /// the join — a PDF shading interpolates between *two* colours per
    /// function, so a three-stop ramp is two functions stitched.
    pieces: Vec<Ref>,
    join: Option<Ref>,
    kind: ShadingKind,
}

/// The geometry of one shading, already in PDF space.
struct ShadingKind {
    axial: bool,
    coords: Vec<f32>,
    stops: Vec<(Vec<f32>, f32)>,
}

/// The distinct shadings the document needs, one per gradient-filled object.
///
/// Not deduplicated, because a shading carries its object's geometry: the same
/// ramp on two differently sized frames is two different sets of coordinates.
/// Lay a document-space path into a PDF content stream.
///
/// The y flip happens here, through `to_pdf_y`, like every other conversion in
/// this file — scattering it is how an exporter comes to disagree with the
/// screen. A path's y flip is the page height minus the y, with no height to
/// subtract, which is what the zero argument says.
fn write_path(content: &mut Content, path: &kurbo::BezPath, page: DocRect) {
    let y = |v: f64| to_pdf_y(page, v, 0.0) as f32;
    for element in path.elements() {
        match element {
            kurbo::PathEl::MoveTo(p) => {
                content.move_to(p.x as f32, y(p.y));
            }
            kurbo::PathEl::LineTo(p) => {
                content.line_to(p.x as f32, y(p.y));
            }
            kurbo::PathEl::CurveTo(a, b, c) => {
                content.cubic_to(a.x as f32, y(a.y), b.x as f32, y(b.y), c.x as f32, y(c.y));
            }
            kurbo::PathEl::QuadTo(a, b) => {
                // kurbo does not produce these here, but a path arriving with
                // one must not be silently dropped into a gap in the outline.
                let (a, b) = (*a, *b);
                content.cubic_to(a.x as f32, y(a.y), a.x as f32, y(a.y), b.x as f32, y(b.y));
            }
            kurbo::PathEl::ClosePath => {
                content.close_path();
            }
        }
    }
}

/// One spot ink, as the plate it will be printed on.
struct Plate {
    /// The colour space object.
    id: Ref,
    /// The tint transform: how much ink becomes what colour.
    transform: Ref,
    resource: String,
    separation: crate::separation::Separation,
}

/// Every spot ink the document names, once each.
///
/// **Keyed on the ink's name**, because that is what a plate *is*. Two objects
/// in the same spot at different tints are one plate at two strengths, and
/// writing two colour spaces for them would tell the press to mount the same
/// ink twice.
fn collect_plates(
    resolved: &ResolvedDocument,
    ink: &Ink,
    alloc: &mut impl FnMut() -> Ref,
) -> Vec<Plate> {
    let mut out: Vec<Plate> = Vec::new();

    let mut note = |colour: &tessera_color::Color, out: &mut Vec<Plate>| {
        let Some(separation) = crate::separation::spots_in(colour, ink) else {
            return;
        };
        if out.iter().any(|p| p.separation.name == separation.name) {
            return;
        }
        out.push(Plate {
            id: alloc(),
            transform: alloc(),
            resource: format!("Sep{}", out.len()),
            separation,
        });
    };

    for item in &resolved.items {
        // Fills, strokes and every stop of every gradient. A spot used only in
        // the middle of a ramp is still an ink somebody has to buy.
        // A path's fill is optional; a rectangle's and an ellipse's are not.
        let (fill, stroke) = match &item.kind {
            ResolvedKind::Rectangle { fill, stroke, .. }
            | ResolvedKind::Ellipse { fill, stroke } => (Some(fill.clone()), stroke.as_ref()),
            ResolvedKind::Path { fill, stroke, .. } => (fill.clone(), stroke.as_ref()),
            _ => (None, None),
        };
        if let Some(fill) = fill {
            for colour in fill.colours() {
                note(&colour, &mut out);
            }
        }
        if let Some(stroke) = stroke {
            note(&stroke.color, &mut out);
        }
    }

    out
}

/// Write each plate: its tint transform, then the space that names it.
fn write_plates(pdf: &mut Pdf, plates: &[Plate]) {
    for plate in plates {
        // An exponential interpolation from no ink to full ink, with N = 1 —
        // which is a straight line. A spot at forty per cent is forty per cent
        // of the way from the paper to the ink, and anything else would be this
        // exporter inventing a dot-gain curve it has no measurements for.
        let none = plate.separation.alternate.none();
        let full = plate.separation.alternate.full();
        let mut function = pdf.exponential_function(plate.transform);
        function
            .domain([0.0, 1.0])
            .c0(none.iter().copied())
            .c1(full.iter().copied())
            .n(1.0);
        function.finish();

        let mut space = pdf.indirect(plate.id).array();
        space.item(Name(b"Separation"));
        space.item(Name(plate.separation.name.as_bytes()));
        space.item(match plate.separation.alternate {
            crate::separation::Alternate::Rgb(_) => Name(b"DeviceRGB"),
            crate::separation::Alternate::Cmyk(_) => Name(b"DeviceCMYK"),
        });
        space.item(plate.transform);
        space.finish();
    }
}

/// One frame's shadow: a rectangle of its colour, masked by its softness.
///
/// **Not a luminosity soft mask.** That is the other way to do this and it needs
/// a transparency group and an `/ExtGState` to hang it on. An image of the
/// shadow's colour carrying an `/SMask` is the same picture with none of that,
/// and it reuses the path placed artwork already goes through — which is the
/// path that is already tested.
struct CastShadow {
    id: Ref,
    mask_id: Ref,
    resource: String,
    mask: crate::shadow::Mask,
    /// Where the mask sits in document space, and how big.
    at: DocRect,
    colour: tessera_color::Color,
}

/// Build a mask for every frame that casts a shadow.
///
/// **Indexed to match `resolved.items`**, as the shadings are, so the content
/// builder asks for item `n`'s shadow rather than searching for it. `None` means
/// the frame casts none, or has no size to cast one from.
fn collect_shadows(
    resolved: &ResolvedDocument,
    alloc: &mut impl FnMut() -> Ref,
) -> Vec<Option<CastShadow>> {
    let mut out: Vec<Option<CastShadow>> = Vec::with_capacity(resolved.items.len());

    for item in &resolved.items {
        let at_index = out.len();
        let made = item.shadow.as_ref().and_then(|shadow| {
            let mask = crate::shadow::mask(shadow, item.bounds.width, item.bounds.height)?;
            let bleed = crate::shadow::bleed(shadow);
            Some(CastShadow {
                id: alloc(),
                mask_id: alloc(),
                resource: format!("Sh{at_index}"),
                mask,
                // Displaced by the shadow's offset, and grown by the blur's
                // reach on every side: the mask is bigger than the shape, or its
                // edge would be hard.
                at: DocRect {
                    x: item.bounds.x + shadow.offset.0 - bleed,
                    y: item.bounds.y + shadow.offset.1 - bleed,
                    width: item.bounds.width + bleed * 2.0,
                    height: item.bounds.height + bleed * 2.0,
                },
                colour: shadow.colour.clone(),
            })
        });
        out.push(made);
    }

    out
}

/// Write each shadow as a flat colour image wearing its softness as a mask.
fn write_shadows(pdf: &mut Pdf, shadows: &[Option<CastShadow>]) {
    for shadow in shadows.iter().flatten() {
        // The softness. One component per sample and `/DeviceGray`, because a
        // soft mask is coverage rather than colour — three components would be
        // read as a third of the pixels.
        //
        // The shadow's own alpha is folded in here rather than written as an
        // `/ExtGState`: coverage and opacity multiply, so doing it once in the
        // mask is the same result with one object instead of two.
        let [r, g, b, alpha] = shadow.colour.to_rgb_f32();
        let alpha = alpha.clamp(0.0, 1.0);
        let scaled: Vec<u8> = shadow
            .mask
            .coverage
            .iter()
            .map(|c| (f32::from(*c) * alpha).round() as u8)
            .collect();

        let packed = crate::images::deflate(&scaled);
        let mut mask = pdf.image_xobject(shadow.mask_id, &packed);
        mask.width(shadow.mask.width as i32)
            .height(shadow.mask.height as i32);
        mask.color_space().device_gray();
        mask.bits_per_component(8).filter(Filter::FlateDecode);
        mask.finish();

        // The colour. A flat field the size of the mask, which deflates to
        // almost nothing — a constant is the best case a compressor has.
        let [r, g, b] = [
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        ];
        let mut flat = Vec::with_capacity(shadow.mask.coverage.len() * 3);
        for _ in 0..shadow.mask.coverage.len() {
            flat.extend_from_slice(&[r, g, b]);
        }

        let packed_flat = crate::images::deflate(&flat);
        let mut image = pdf.image_xobject(shadow.id, &packed_flat);
        image
            .width(shadow.mask.width as i32)
            .height(shadow.mask.height as i32);
        image.color_space().device_rgb();
        image.bits_per_component(8).filter(Filter::FlateDecode);
        image.s_mask(shadow.mask_id);
        image.finish();
    }
}

/// One placed picture, written once however many frames show it.
struct Picture {
    /// The file it came from, which is what makes it reusable.
    source: std::path::PathBuf,
    id: Ref,
    /// The `/SMask` object, when the artwork has an alpha channel.
    mask: Option<Ref>,
    resource: String,
    ready: crate::images::Prepared,
}

/// Read every placed file once, whatever it is placed into.
///
/// **Keyed on the path.** A logo on forty pages is one image object and forty
/// references to it; embedding it forty times would multiply the file by forty
/// for a picture the reader already has.
///
/// A file that cannot be read is **skipped, not fatal**. Preflight has already
/// reported the broken link, and refusing the whole export because of one
/// missing picture would mean a job with a broken link cannot even be proofed.
fn collect_pictures(
    resolved: &ResolvedDocument,
    alloc: &mut impl FnMut() -> Ref,
    ink: &Ink,
) -> Vec<Picture> {
    let mut out: Vec<Picture> = Vec::new();

    for item in &resolved.items {
        let ResolvedKind::Graphic { source, .. } = &item.kind else {
            continue;
        };
        let Some(source) = source else { continue };
        if out.iter().any(|p| &p.source == source) {
            continue;
        }
        // Converted through the press's own profile when there is one, so a
        // CMYK export has no RGB left in it. The pass-through that makes an RGB
        // export cheap is exactly what a converting export cannot have, and
        // that is a real cost rather than a shortcut worth looking for.
        let ready = match ink {
            Ink::Cmyk(conversion) => crate::images::to_cmyk(source, conversion),
            Ink::Rgb => crate::images::prepare(source),
        };
        let Ok(ready) = ready else {
            continue;
        };

        let id = alloc();
        let mask = ready.is_transparent().then(&mut *alloc);
        out.push(Picture {
            source: source.clone(),
            id,
            mask,
            resource: format!("Im{}", out.len()),
            ready,
        });
    }

    out
}

/// Write the image objects themselves.
fn write_pictures(pdf: &mut Pdf, pictures: &[Picture]) {
    use crate::images::Coding;

    for picture in pictures {
        let ready = &picture.ready;

        // The mask first, so its id is settled before the image names it.
        if let (Some(mask_id), Some(alpha)) = (picture.mask, ready.alpha.as_ref()) {
            let mut mask = pdf.image_xobject(mask_id, alpha);
            mask.width(ready.width as i32)
                .height(ready.height as i32)
                // One component per pixel, and **`/DeviceGray`**: a soft mask is
                // coverage, not colour. Written in a colour space with three
                // components it would be read as a third of the pixels.
                .color_space()
                .device_gray();
            mask.bits_per_component(8).filter(Filter::FlateDecode);
            mask.finish();
        }

        let mut image = pdf.image_xobject(picture.id, &ready.data);
        image.width(ready.width as i32).height(ready.height as i32);
        match ready.space {
            crate::images::Space::Cmyk => image.color_space().device_cmyk(),
            crate::images::Space::Rgb => image.color_space().device_rgb(),
        };
        image.bits_per_component(8);
        image.filter(match ready.coding {
            // `/DCTDecode` *is* JPEG: the file's own bytes, handed over.
            Coding::Jpeg => Filter::DctDecode,
            Coding::Flate => Filter::FlateDecode,
        });
        if let Some(mask_id) = picture.mask {
            image.s_mask(mask_id);
        }
        image.finish();
    }
}

fn collect_shadings(
    resolved: &ResolvedDocument,
    page: DocRect,
    alloc: &mut impl FnMut() -> Ref,
    ink: &Ink,
) -> Vec<Option<Shading>> {
    let mut out = Vec::with_capacity(resolved.items.len());

    for item in &resolved.items {
        let fill = match &item.kind {
            ResolvedKind::Rectangle { fill, .. } | ResolvedKind::Ellipse { fill, .. } => Some(fill),
            ResolvedKind::Path { fill, .. } => fill.as_ref(),
            _ => None,
        };
        let Some(gradient) = fill.and_then(|f| f.gradient()) else {
            out.push(None);
            continue;
        };

        let (from, to) = gradient.axis(item.bounds);
        let axial = matches!(gradient.ramp, tessera_document::paint::Ramp::Linear { .. });
        // Into PDF space, where the origin is at the bottom. The same flip the
        // shapes get, so the ramp cannot end up running the other way from the
        // object it fills.
        let flip = |y: f64| to_pdf_y(page, y, 0.0) as f32;
        let coords = if axial {
            vec![from.x as f32, flip(from.y), to.x as f32, flip(to.y)]
        } else {
            // Centre, inner radius, centre, outer radius: a PDF radial shading
            // is between two circles, and a plain radial ramp is the degenerate
            // case where the inner one is a point.
            vec![
                from.x as f32,
                flip(from.y),
                0.0,
                from.x as f32,
                flip(from.y),
                gradient.radius(item.bounds) as f32,
            ]
        };

        // In whichever space this export writes. A shading declares its colour
        // space once and every function under it must agree, so a ramp in a CMYK
        // export is four components per stop and not three.
        let stops: Vec<(Vec<f32>, f32)> = gradient
            .stops()
            .iter()
            .map(|stop| (ink.components(&stop.colour).values(), stop.at))
            .collect();

        let pieces: Vec<Ref> = (0..stops.len().saturating_sub(1))
            .map(|_| alloc())
            .collect();
        // A single interval needs no stitching: the one exponential function
        // *is* the ramp, and wrapping it would be a dictionary describing
        // nothing.
        let join = if pieces.len() > 1 {
            Some(alloc())
        } else {
            None
        };

        let next = out
            .iter()
            .filter(|s: &&Option<Shading>| s.is_some())
            .count();
        out.push(Some(Shading {
            resource: format!("Sh{next}"),
            id: alloc(),
            pieces,
            join,
            kind: ShadingKind {
                axial,
                coords,
                stops,
            },
        }));
    }

    out
}

fn write_shading(pdf: &mut Pdf, shading: &Shading, ink: &Ink) {
    use pdf_writer::types::FunctionShadingType;

    // One exponential function per interval, each interpolating between the two
    // colours at its ends. `N = 1` is a straight ramp; anything else would be a
    // curve nobody asked for.
    for (i, piece) in shading.pieces.iter().enumerate() {
        let (from, _) = &shading.kind.stops[i];
        let (to, _) = &shading.kind.stops[i + 1];
        let mut function = pdf.exponential_function(*piece);
        function
            .domain([0.0, 1.0])
            .c0(from.iter().copied())
            .c1(to.iter().copied())
            .n(1.0);
        function.finish();
    }

    if let Some(join) = shading.join {
        // Where each interval starts, in the ramp's own 0..1 domain. The first
        // and last stop positions are the domain's ends and so are not bounds.
        let bounds: Vec<f32> = shading.kind.stops[1..shading.kind.stops.len() - 1]
            .iter()
            .map(|(_, at)| *at)
            .collect();
        let encode: Vec<f32> = shading.pieces.iter().flat_map(|_| [0.0f32, 1.0]).collect();
        let mut function = pdf.stitching_function(join);
        function
            .domain([0.0, 1.0])
            .functions(shading.pieces.iter().copied())
            .bounds(bounds)
            .encode(encode);
        function.finish();
    }

    let mut written = pdf.function_shading(shading.id);
    written.shading_type(if shading.kind.axial {
        FunctionShadingType::Axial
    } else {
        FunctionShadingType::Radial
    });
    // The same space the stops were written in, or no RIP will read the file.
    if ink.is_cmyk() {
        written.color_space().device_cmyk();
    } else {
        written.color_space().device_rgb();
    }
    written
        .coords(shading.kind.coords.iter().copied())
        // Extended at both ends, so the first and last colours run to the edge
        // of the shape rather than leaving it unpainted where the ramp stops.
        .extend([true, true])
        .function(shading.join.unwrap_or(shading.pieces[0]));
    written.finish();
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
                .runs()
                .filter(|r| r.font_index == index)
                .flat_map(|r| r.glyphs.iter().map(move |g| (r.size, g)))
            {
                let (size, glyph) = glyph;
                let id = u16::try_from(glyph.glyph_id)
                    .map_err(|_| PdfError::GlyphIdTooLarge(glyph.glyph_id))?;
                used[slot].1.push(id);
                // Advance is carried from the shaper in points at the size
                // **its own run** was shaped at. Dividing by any other size
                // gives a PDF whose text sits correctly and whose widths are
                // wrong — which a viewer will not complain about and a
                // printer will.
                let width = glyph.advance / f64::from(size) * PDF_UNITS_PER_EM;
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

/// Everything the content stream needs besides the document itself.
///
/// One argument rather than seven. They arrived one at a time as milestone 6
/// grew, and a function whose parameter list has to be read carefully to call is
/// one where a caller eventually passes the fonts where the shadings go.
struct Written<'a> {
    page: DocRect,
    fonts: &'a [EmbeddedFont],
    states: &'a [GraphicsState],
    shadings: &'a [Option<Shading>],
    pictures: &'a [Picture],
    shadows: &'a [Option<CastShadow>],
    plates: &'a [Plate],
    ink: &'a Ink,
    resolved_page: &'a tessera_layout::ResolvedPage,
    options: &'a ExportOptions,
}

fn build_content(resolved: &ResolvedDocument, w: &Written<'_>) -> Result<Vec<u8>, PdfError> {
    let Written {
        page,
        fonts,
        states,
        shadings,
        pictures,
        shadows,
        plates,
        ink,
        resolved_page,
        options,
    } = *w;
    let mut content = Content::new();

    for (index, item) in resolved.items.iter().enumerate() {
        let shading = shadings.get(index).and_then(|s| s.as_ref());
        // An object at no opacity is not written, exactly as it is not drawn.
        // Writing it at `/ca 0` would put ink-free paint in the file for a
        // press to process and a viewer to composite, for no visible result.
        if item.blend.is_invisible() {
            continue;
        }

        // A placed item gets its own graphics state, with its transform
        // written as a `cm` matrix.
        let placed = !item.transform.is_identity();
        if placed {
            content.save_state();
            content.transform(to_pdf_matrix(item.transform, page).map(|v| v as f32));
        }

        // The object's compositing, named from the page's resources. Set
        // outside the per-kind save/restore so that the fill and the stroke it
        // brackets both inherit it.
        let composited = states.iter().find(|s| {
            !item.blend.is_plain()
                && s.blend.mode == item.blend.mode
                && s.blend.alpha().to_bits() == item.blend.alpha().to_bits()
        });
        if let Some(state) = composited {
            content.save_state();
            content.set_parameters(Name(state.resource.as_bytes()));
        }

        // The shadow, before anything the frame itself draws. It is behind the
        // shape by definition, and drawing it after would put it over the fill.
        if let Some(shadow) = shadows.get(index).and_then(|s| s.as_ref()) {
            content.save_state();
            let b = shadow.at;
            content.transform([
                b.width as f32,
                0.0,
                0.0,
                b.height as f32,
                b.x as f32,
                to_pdf_y(page, b.y, b.height) as f32,
            ]);
            content.x_object(Name(shadow.resource.as_bytes()));
            content.restore_state();
        }

        match &item.kind {
            // The picture, if its file could be read. The placeholder is
            // still never written: a picture box is **furniture** — the cross
            // and the frame edge are interface, not ink — and a violet cross in
            // a printed job is far worse than a blank space.
            ResolvedKind::Graphic { source, .. } => {
                if let Some(source) = source
                    && let Some(picture) = pictures.iter().find(|p| &p.source == source)
                {
                    content.save_state();
                    // A PDF image occupies the **unit square**, so the matrix is
                    // the whole of where and how big it is. The y flip is part
                    // of it: PDF's image space runs top-down inside a
                    // bottom-up page, so without the negative height every
                    // photograph would print upside down.
                    let b = item.bounds;
                    content.transform([
                        b.width as f32,
                        0.0,
                        0.0,
                        b.height as f32,
                        b.x as f32,
                        to_pdf_y(page, b.y, b.height) as f32,
                    ]);
                    content.x_object(Name(picture.resource.as_bytes()));
                    content.restore_state();
                }
            }

            ResolvedKind::Rectangle {
                fill,
                stroke,
                outline,
            } => {
                content.save_state();
                // Cut corners come as the same path the renderer draws, laid
                // into PDF space. Square corners stay a `re` operator: it is
                // one token against a dozen, and it is the commonest shape on
                // any page.
                let shape = |c: &mut Content, b: DocRect| match outline {
                    Some(path) => write_path(c, path, page),
                    None => {
                        c.rect(
                            b.x as f32,
                            to_pdf_y(page, b.y, b.height) as f32,
                            b.width as f32,
                            b.height as f32,
                        );
                    }
                };
                let rect = |c: &mut Content, b: DocRect| {
                    c.rect(
                        b.x as f32,
                        to_pdf_y(page, b.y, b.height) as f32,
                        b.width as f32,
                        b.height as f32,
                    );
                };

                match shading {
                    // A gradient is painted by clipping to the shape and
                    // running the shading over it, which is what `sh` does: it
                    // fills the current clip, so the clip *is* the shape.
                    Some(sh) => {
                        content.save_state();
                        shape(&mut content, item.bounds);
                        content.clip_nonzero();
                        content.end_path();
                        content.shading(Name(sh.resource.as_bytes()));
                        content.restore_state();
                    }
                    None => {
                        set_solid_fill(&mut content, fill, ink, plates);
                        shape(&mut content, item.bounds);
                        content.fill_nonzero();
                    }
                }

                if let Some(s) = stroke {
                    // The fill and the stroke follow different rectangles once
                    // the stroke is aligned inside or outside, so they cannot
                    // share one path.
                    apply_stroke(&mut content, s, ink, plates);
                    match outline {
                        // A cut corner strokes on its own centre line: offsetting
                        // a curved path is an offset curve, which is not a bezier
                        // and cannot be had by moving control points. The renderer
                        // makes the same compromise, so the two still agree.
                        Some(path) => write_path(&mut content, path, page),
                        None => rect(&mut content, offset_rect(item.bounds, s.offset())),
                    }
                    content.stroke();
                }
                content.restore_state();
            }

            ResolvedKind::Ellipse { fill, stroke } => {
                content.save_state();
                match shading {
                    Some(sh) => {
                        content.save_state();
                        ellipse_path(&mut content, page, item.bounds);
                        content.clip_nonzero();
                        content.end_path();
                        content.shading(Name(sh.resource.as_bytes()));
                        content.restore_state();
                    }
                    None => {
                        set_solid_fill(&mut content, fill, ink, plates);
                        ellipse_path(&mut content, page, item.bounds);
                        content.fill_nonzero();
                    }
                }
                if let Some(s) = stroke {
                    apply_stroke(&mut content, s, ink, plates);
                    ellipse_path(&mut content, page, offset_rect(item.bounds, s.offset()));
                    content.stroke();
                }
                content.restore_state();
            }

            ResolvedKind::Path { path, fill, stroke } => {
                content.save_state();
                emit_path(&mut content, page, item.bounds, path);
                match (fill, stroke) {
                    (Some(_), _) if shading.is_some() => {
                        let sh = shading.expect("just checked");
                        content.clip_nonzero();
                        content.end_path();
                        content.shading(Name(sh.resource.as_bytes()));
                    }
                    (Some(f), _) => {
                        set_solid_fill(&mut content, f, ink, plates);
                        content.fill_nonzero();
                    }
                    (None, Some(s)) => {
                        apply_stroke(&mut content, s, ink, plates);
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
                draw_text(&mut content, page, item.bounds, shaped, color, fonts, ink)?;
            }
        }

        if composited.is_some() {
            content.restore_state();
        }
        if placed {
            content.restore_state();
        }
    }

    // The marks, after the document and outside the trim, so nothing on the page
    // can sit on top of a crop mark. An object dragged onto the pasteboard is
    // not bounded by the bleed, and a crop mark half covered by a stray
    // rectangle is one somebody cuts to the wrong place.
    crate::marks::draw(&mut content, resolved_page, options, ink);

    Ok(content.finish().to_vec())
}

/// Set every stroke attribute on the content stream.
///
/// Colour, width, cap, join, miter limit and dash pattern. A stroke that
/// exported as a bare width would not be the stroke that was on screen, which
/// is the one thing this crate exists to prevent.
fn apply_stroke(content: &mut Content, stroke: &Stroke, ink: &Ink, plates: &[Plate]) {
    match plate_for(&stroke.color, plates) {
        Some(plate) => {
            let tint = crate::separation::tint_of(&stroke.color).unwrap_or(1.0);
            content.set_stroke_color_space(pdf_writer::types::ColorSpaceOperand::Named(Name(
                plate.resource.as_bytes(),
            )));
            content.set_stroke_color([tint]);
        }
        None => ink.set_stroke(content, &stroke.color),
    }
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
/// Set the fill colour from a paint that is a solid one.
///
/// A gradient never reaches here — the arms that paint one take the shading
/// path instead — so this is only for a paint that says it is solid. It falls
/// back to the ramp’s representative colour rather than to black, so that if a
/// gradient ever did arrive the page would be wrong in a way somebody notices
/// rather than silently black.
fn set_solid_fill(
    content: &mut Content,
    paint: &tessera_document::paint::Paint,
    ink: &Ink,
    plates: &[Plate],
) {
    let colour = paint
        .solid()
        .cloned()
        .unwrap_or_else(|| paint.representative());
    // A spot goes on its own plate: `/Sep0 cs 0.4 scn` rather than the process
    // mix that approximates it. The approximation is still in the file, as the
    // separation's tint transform, for anything that cannot print the real ink.
    if let Some(plate) = plate_for(&colour, plates) {
        let tint = crate::separation::tint_of(&colour).unwrap_or(1.0);
        content.set_fill_color_space(pdf_writer::types::ColorSpaceOperand::Named(Name(
            plate.resource.as_bytes(),
        )));
        content.set_fill_color([tint]);
        return;
    }
    ink.set_fill(content, &colour);
}

/// The plate a colour belongs to, if it names a spot ink.
fn plate_for<'a>(colour: &tessera_color::Color, plates: &'a [Plate]) -> Option<&'a Plate> {
    let tessera_color::Color::Spot { name, .. } = colour else {
        return None;
    };
    plates.iter().find(|p| &p.separation.name == name)
}

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
    ink: &Ink,
) -> Result<(), PdfError> {
    // One text object per run, because the size lives there — and now the
    // colour too. Grouping by font alone would set the font once and draw
    // every size at it.
    for run in shaped.runs() {
        let run_colour = run.colour.as_ref().unwrap_or(color).clone();
        let index = run.font_index;
        // Match by subset content: `collect_fonts` walked the same items in
        // the same order, so position `index` here maps to the same font.
        let Some(embedded) = fonts.get(font_for(shaped, index, fonts)) else {
            continue;
        };
        if run.glyphs.is_empty() {
            continue;
        }

        content.save_state();
        ink.set_fill(content, &run_colour);
        content.begin_text();
        content.set_font(Name(embedded.resource.as_bytes()), run.size);

        for glyph in run.glyphs.iter() {
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
