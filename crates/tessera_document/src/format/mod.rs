//! The `.tessera` container.
//!
//! A zip archive holding the serialized document, its metadata, and — from
//! later milestones — a thumbnail and embedded assets:
//!
//! ```text
//! document.json   the serialized document model
//! meta.json       format version, application version, timestamps
//! thumbnail.png   first spread            (milestone 3)
//! links.json      linked-asset manifest   (milestone 5)
//! fonts/ links/   embedded, when packaged (milestone 6)
//! ```
//!
//! A container rather than a bare JSON file so that packaging — collecting
//! links and fonts into one deliverable — is the *same mechanism* as saving,
//! so a thumbnail can be read without parsing the document, and so recovery
//! tooling can pull `document.json` straight out of a damaged file.

pub mod meta;

use std::io::{Cursor, Read, Write};
use std::path::Path;

pub use meta::Meta;

use tessera_geometry::{DocPoint, Transform};

use crate::document::Document;

/// Bumped whenever the on-disk shape changes. An older version runs
/// migrations; a newer one is refused rather than guessed at.
pub const FORMAT_VERSION: u32 = 14;

const DOCUMENT_ENTRY: &str = "document.json";
const META_ENTRY: &str = "meta.json";

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("could not read {0}")]
    Read(std::path::PathBuf),
    #[error("the archive is missing {0}")]
    MissingEntry(&'static str),
    #[error("the file is not a valid Tessera document: {0}")]
    Archive(String),
    #[error("could not parse {entry}: {source}")]
    Parse {
        entry: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "this document was saved by a newer version of Tessera \
         (format {found}, this build supports {supported})"
    )]
    NewerFormat { found: u32, supported: u32 },
    #[error(transparent)]
    Io(#[from] tessera_io::atomic::IoError),
    #[error("could not build the archive: {0}")]
    Write(String),
}

pub fn save(doc: &Document, path: &Path) -> Result<(), FormatError> {
    let bytes = to_bytes(doc, FORMAT_VERSION)?;
    tessera_io::atomic::write_atomic(path, &bytes)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Document, FormatError> {
    let bytes = std::fs::read(path).map_err(|_| FormatError::Read(path.to_path_buf()))?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| FormatError::Archive(e.to_string()))?;

    let meta: Meta = read_json(&mut zip, META_ENTRY)?;
    if meta.format_version > FORMAT_VERSION {
        return Err(FormatError::NewerFormat {
            found: meta.format_version,
            supported: FORMAT_VERSION,
        });
    }

    let mut value: serde_json::Value = read_json(&mut zip, DOCUMENT_ENTRY)?;
    migrate(&mut value, meta.format_version);
    serde_json::from_value(value).map_err(|source| FormatError::Parse {
        entry: DOCUMENT_ENTRY,
        source,
    })
}

/// Bring a document forward from `from` to [`FORMAT_VERSION`], one step at a
/// time.
///
/// Stepwise rather than a single jump, so a document written three versions
/// ago follows exactly the path a document written two versions ago does, and
/// each step only has to know about its own change.
fn migrate(value: &mut serde_json::Value, from: u32) {
    // 1 -> 2: frames gained `rotation`. The field carried `serde(default)`,
    // so an absent value already read as 0.0 and there was nothing to
    // rewrite. Superseded by the next step, which removes the field entirely.

    // 2 -> 3: `rotation` became a full affine `transform`.
    //
    // The first migration that actually rewrites. A frame's rotation was
    // always about its own centre, so the equivalent transform is exactly
    // that rotation — `Transform::rotate_about` is asserted to be the same
    // operation as the `DocPoint::rotated_about` frames used before it.
    if from < 3 {
        rotation_to_transform(value);
    }

    // 3 -> 4: strokes gained alignment, caps, joins, a miter limit and
    // dashes. Every one of them carries `serde(default)`, and the defaults
    // are what a stroke drew before they existed, so there is nothing to
    // rewrite. The version still moves, so that a build without them refuses
    // a document that uses them rather than silently dropping them on the
    // next save.

    // 4 -> 5: the document gained page setup — margins, bleed, slug, and
    // whether pages face — and a spread gained guides.
    //
    // One bump for all of it, deliberately. A format version costs a
    // migration test, so six changes delivered together cost one and six
    // delivered separately cost six. That is why milestone 1.5 has a phase
    // whose whole job is the page.
    //
    // Nothing to rewrite. Every field carries `serde(default)`, and each
    // default is the truth about a document that never had the field: no
    // margins, no bleed, no slug, pages that do not face, no guides. A
    // fabricated default — 10mm margins, say — would invent a decision the
    // user never made, and would do it silently, to every old document.
    //
    // The version moves anyway, for the same reason 3 -> 4 did.

    // 5 -> 6: a story gained character and paragraph runs, so that it can
    // hold more than one formatting.
    //
    // **This one rewrites, and it has to.** The run lists carry an invariant
    // — sorted, non-overlapping, covering exactly the text — and `serde`'s
    // default for a missing list is an empty one. An older story loaded
    // without this step would arrive with text and no runs, which satisfies
    // no version of that invariant. Every other migration so far could lean
    // on a default being the truth about an older document; this is the first
    // where the default is a lie.
    if from < 6 {
        stories_gain_runs(value);
    }

    // 6 -> 7: the document gained named style tables and a text default, and
    // a story lost the single `style` it had carried since milestone 0.
    //
    // The style folds into every run — `run.local` **over** it, so a run that
    // already states a size keeps its own — and then the field goes. Nothing
    // about how a document looks changes, which is the whole bar for a
    // migration that removes something.
    if from < 7 {
        stories_fold_style_into_runs(value);
    }

    // 7 -> 8: layers left the page and became document-wide.
    //
    // **This one rewrites, and the default would be a lie again.** An absent
    // `layer_order` reads as empty, and an empty layer order means a document
    // whose every object is on no layer at all — which paints nothing. The
    // whole file would open blank.
    //
    // Per-page layers merge **by position**: layer 0 of every page becomes one
    // document-wide layer 0, layer 1 becomes layer 1, and so on. For every
    // document that exists today that is a single layer holding everything,
    // because nothing before this could create a second one — and that is the
    // truth about a document written when layers could not be chosen.
    if from < 8 {
        layers_leave_the_page(value);
    }

    // 13 -> 14: a frame gained its compositing — an opacity and a blend mode
    // of its own, distinct from its fill colour's alpha.
    //
    // Nothing to rewrite. `Blending::PLAIN` — fully opaque, painted over — is
    // exactly what every object written before this did, so the default is the
    // truth rather than a fabrication. The version moves so an older build
    // refuses a document using opacity rather than opening it with every
    // watermark and every multiplied shadow drawn flat and solid, which would
    // look like the document rather than like the build.

    // 12 -> 13: the document gained a table of links, and a frame gained the
    // kind that shows one.
    //
    // Nothing to rewrite. An absent link table is an empty one, which is the
    // truth about a document written before artwork could be placed, and no
    // frame in it can be a graphic one because the variant did not exist to be
    // written. The version moves so an older build refuses a document with
    // pictures in it rather than opening it with the picture boxes silently
    // absent — which would look like the document rather than like the build.

    // 11 -> 12: the document gained a table of named colours, and `Color`
    // gained Lab and a reference to one of them.
    //
    // Nothing to rewrite: an absent table is an empty one, which is the truth
    // about a document written before swatches existed, and no colour in it
    // can be a reference because the variant did not exist to be written. The
    // version moves so an older build refuses a document using swatches rather
    // than opening it with every swatched object drawn in the unresolved
    // colour.

    // 10 -> 11: the document gained a baseline grid and a text frame gained
    // the switch that locks its lines to it.
    //
    // Nothing to rewrite. `None` is the truth about a document written before
    // grids existed — it has no grid — and `false` is the truth about every
    // frame in it. The version moves so that an older build refuses a document
    // using a grid rather than opening it with every line off the rhythm,
    // which would look like the document rather than like the build.

    // 9 -> 10: a text frame gained the layout it flows its story through —
    // columns, a gutter, an inset, and where the text sits when it does not
    // fill the frame.
    //
    // Nothing to rewrite. `TextLayout::default()` is one column, no inset,
    // aligned to the top, which is exactly what every text frame written
    // before this did. The version moves so that a build without columns
    // refuses a document that uses them rather than opening it with every
    // column silently collapsed into one.

    // 8 -> 9: the document gained parent pages, a page gained the parent it
    // is built on, and the document gained the map of which local frame
    // stands in for which master item.
    //
    // Nothing to rewrite, and this time the defaults really are the truth: a
    // document written before masters existed has no masters, none of its
    // pages is built on one, and nothing in it overrides anything. Every one
    // of those is what an empty collection means.
    //
    // The version moves anyway, so that a build without masters refuses a
    // document that uses them rather than opening it with the furniture
    // silently missing from every page.
}

/// Move layers off the pages and into a document-wide stack.
///
/// Rebuilds the layer table outright rather than editing slots in place, which
/// it can do because **nothing but a page refers to a `LayerId`**: frames point
/// at their children and their stories, never back at a layer.
fn layers_leave_the_page(value: &mut serde_json::Value) {
    use serde_json::{Map, Value};

    let Some(doc) = value.as_object_mut() else {
        return;
    };

    // A slotmap serialises as an array of `{value, version}`, indexed by slot,
    // and a key is `{idx, version}`. Slot 0 is always the vacant sentinel.
    let slot = |table: &Value, key: &Value| -> Option<Value> {
        let idx = key.get("idx")?.as_u64()? as usize;
        table
            .get(idx)?
            .get("value")
            .cloned()
            .filter(|v| !v.is_null())
    };

    let layer_table = doc.get("layers").cloned().unwrap_or(Value::Null);

    // Each page's layers, deepest first, in page order.
    let mut columns: Vec<Vec<Value>> = Vec::new();
    if let Some(pages) = doc.get_mut("pages").and_then(Value::as_array_mut) {
        for page in pages.iter_mut() {
            let Some(page) = page.get_mut("value").and_then(Value::as_object_mut) else {
                continue;
            };
            let Some(keys) = page.remove("layers") else {
                continue;
            };
            let Some(keys) = keys.as_array() else {
                continue;
            };
            for (depth, key) in keys.iter().enumerate() {
                let Some(layer) = slot(&layer_table, key) else {
                    continue;
                };
                while columns.len() <= depth {
                    columns.push(Vec::new());
                }
                columns[depth].push(layer);
            }
        }
    }

    // One layer per depth, holding every page's frames at that depth. The
    // first page's name, visibility and lock stand for the merged layer:
    // they were all "Layer 1", visible and unlocked, because nothing could
    // make them anything else.
    let mut table = vec![serde_json::json!({ "value": null, "version": 0 })];
    let mut order = Vec::new();
    for column in &columns {
        let mut frames = Vec::new();
        for layer in column {
            if let Some(list) = layer.get("frames").and_then(Value::as_array) {
                frames.extend(list.iter().cloned());
            }
        }
        let first = column.first();
        let mut merged = Map::new();
        merged.insert("frames".into(), Value::Array(frames));
        merged.insert(
            "name".into(),
            first
                .and_then(|l| l.get("name").cloned())
                .unwrap_or_else(|| Value::String("Layer 1".into())),
        );
        merged.insert(
            "visible".into(),
            first
                .and_then(|l| l.get("visible").cloned())
                .unwrap_or(Value::Bool(true)),
        );
        merged.insert(
            "locked".into(),
            first
                .and_then(|l| l.get("locked").cloned())
                .unwrap_or(Value::Bool(false)),
        );

        let idx = table.len();
        table.push(serde_json::json!({ "value": Value::Object(merged), "version": 1 }));
        order.push(serde_json::json!({ "idx": idx, "version": 1 }));
    }

    // A document with no layers at all would paint nothing, so it gets one.
    if order.is_empty() {
        table.push(serde_json::json!({
            "value": { "frames": [], "name": "Layer 1", "visible": true, "locked": false },
            "version": 1
        }));
        order.push(serde_json::json!({ "idx": 1, "version": 1 }));
    }

    doc.insert("active_layer".into(), order[0].clone());
    doc.insert("layer_order".into(), Value::Array(order));
    doc.insert("layers".into(), Value::Array(table));
}

/// Fold each story's one style into its runs, then drop it.
fn stories_fold_style_into_runs(value: &mut serde_json::Value) {
    use serde_json::{Map, Value};

    match value {
        Value::Object(map) => {
            if map.contains_key("text") && map.contains_key("style") {
                let mut base = Map::new();
                if let Some(style) = map.get("style").and_then(Value::as_object) {
                    for (from, to) in [
                        ("family", "family"),
                        ("size", "size"),
                        ("line_height", "line_height"),
                        ("color", "colour"),
                    ] {
                        if let Some(v) = style.get(from) {
                            base.insert(to.to_string(), v.clone());
                        }
                    }
                }

                if let Some(runs) = map.get_mut("runs").and_then(Value::as_array_mut) {
                    for run in runs {
                        let Some(run) = run.as_object_mut() else {
                            continue;
                        };
                        let local = run
                            .entry("local")
                            .or_insert_with(|| Value::Object(Map::new()));
                        if let Some(local) = local.as_object_mut() {
                            // The run's own wins; the style fills the gaps.
                            for (k, v) in &base {
                                local.entry(k.clone()).or_insert_with(|| v.clone());
                            }
                        }
                    }
                }

                map.remove("style");
            }

            for child in map.values_mut() {
                stories_fold_style_into_runs(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                stories_fold_style_into_runs(item);
            }
        }
        _ => {}
    }
}

/// Give every story one run and one paragraph, from the style it already had.
///
/// Walks the whole tree rather than reaching for `stories`, for the same
/// reason [`rotation_to_transform`] does: how `SlotMap` encodes its slots is
/// its business. A story is recognised by carrying `text` and `style`
/// together and no `runs` of its own.
fn stories_gain_runs(value: &mut serde_json::Value) {
    use serde_json::{Map, Value};

    match value {
        Value::Object(map) => {
            let is_story =
                map.contains_key("text") && map.contains_key("style") && !map.contains_key("runs");

            if is_story {
                let length = map
                    .get("text")
                    .and_then(Value::as_str)
                    .map_or(0, |t| t.len());

                // The old single style becomes the one run's formatting, so
                // nothing about how the story looks changes.
                let mut local = Map::new();
                if let Some(style) = map.get("style").and_then(Value::as_object) {
                    for (from, to) in [
                        ("family", "family"),
                        ("size", "size"),
                        ("line_height", "line_height"),
                        ("color", "colour"),
                    ] {
                        if let Some(v) = style.get(from) {
                            local.insert(to.to_string(), v.clone());
                        }
                    }
                }

                let (runs, paragraphs) = if length == 0 {
                    // An empty story has no runs, not one empty run.
                    (Value::Array(vec![]), Value::Array(vec![]))
                } else {
                    let range = serde_json::json!({ "start": 0, "end": length });
                    (
                        serde_json::json!([{ "range": range, "local": local }]),
                        serde_json::json!([{ "range": range }]),
                    )
                };

                map.insert("runs".to_string(), runs);
                map.insert("paragraphs".to_string(), paragraphs);
            }

            for child in map.values_mut() {
                stories_gain_runs(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                stories_gain_runs(item);
            }
        }
        _ => {}
    }
}

/// Rewrite every `{ bounds, rotation }` object into `{ bounds, transform }`.
///
/// Walks the whole tree rather than reaching for a known path, because how
/// `SlotMap` chooses to encode its slots is its business and not something the
/// migration should depend on. A frame is recognised by carrying both keys.
fn rotation_to_transform(value: &mut serde_json::Value) {
    use serde_json::Value;

    match value {
        Value::Object(map) => {
            let rewritten = map
                .get("rotation")
                .and_then(Value::as_f64)
                .zip(map.get("bounds").and_then(centre_of))
                .map(|(degrees, centre)| Transform::rotate_about(degrees, centre));

            if let Some(transform) = rewritten {
                map.remove("rotation");
                map.insert(
                    "transform".to_string(),
                    serde_json::to_value(transform).unwrap_or(Value::Null),
                );
            }
            for child in map.values_mut() {
                rotation_to_transform(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                rotation_to_transform(item);
            }
        }
        _ => {}
    }
}

/// The centre of a serialized `DocRect`, if it really is one.
fn centre_of(bounds: &serde_json::Value) -> Option<DocPoint> {
    let read = |key: &str| bounds.get(key)?.as_f64();
    Some(DocPoint {
        x: read("x")? + read("width")? / 2.0,
        y: read("y")? + read("height")? / 2.0,
    })
}

fn to_bytes(doc: &Document, version: u32) -> Result<Vec<u8>, FormatError> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default();

        let mut meta = Meta::current();
        meta.format_version = version;
        let meta_bytes = serde_json::to_vec_pretty(&meta).map_err(|source| FormatError::Parse {
            entry: META_ENTRY,
            source,
        })?;
        zip.start_file(META_ENTRY, options)
            .map_err(|e| FormatError::Write(e.to_string()))?;
        zip.write_all(&meta_bytes)
            .map_err(|e| FormatError::Write(e.to_string()))?;

        let body = serde_json::to_vec_pretty(doc).map_err(|source| FormatError::Parse {
            entry: DOCUMENT_ENTRY,
            source,
        })?;
        zip.start_file(DOCUMENT_ENTRY, options)
            .map_err(|e| FormatError::Write(e.to_string()))?;
        zip.write_all(&body)
            .map_err(|e| FormatError::Write(e.to_string()))?;

        zip.finish()
            .map_err(|e| FormatError::Write(e.to_string()))?;
    }
    Ok(buffer.into_inner())
}

fn read_json<T: serde::de::DeserializeOwned>(
    zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    entry: &'static str,
) -> Result<T, FormatError> {
    let mut file = zip
        .by_name(entry)
        .map_err(|_| FormatError::MissingEntry(entry))?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|_| FormatError::MissingEntry(entry))?;
    serde_json::from_str(&text).map_err(|source| FormatError::Parse { entry, source })
}

/// Rewrites only the version field, so the refusal path can be tested without
/// hand-building an archive.
#[doc(hidden)]
pub fn rewrite_version_for_test(path: &Path, version: u32) -> Result<(), FormatError> {
    let doc = load(path)?;
    let bytes = to_bytes(&doc, version)?;
    tessera_io::atomic::write_atomic(path, &bytes)?;
    Ok(())
}

#[cfg(test)]
mod precision_tests {
    use tessera_geometry::DocRect;

    /// The exact value the round-trip property test rejected on its first run.
    ///
    /// serde_json parses floats to best-effort precision unless the
    /// `float_roundtrip` feature is enabled. Without it this value came back
    /// as 198.1286003255392: a document silently altered simply by loading
    /// it. These tests exist so that feature can never be dropped unnoticed.
    const AWKWARD: f64 = 198.12860032553922;

    #[test]
    fn json_preserves_an_f64_exactly() {
        let s = serde_json::to_string(&AWKWARD).expect("ser");
        let back: f64 = serde_json::from_str(&s).expect("de");
        assert_eq!(AWKWARD, back, "serialized as {s}");
    }

    #[test]
    fn a_doc_rect_preserves_its_geometry_exactly() {
        let r = DocRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: AWKWARD,
        };
        let s = serde_json::to_vec_pretty(&r).expect("ser");
        let back: DocRect = serde_json::from_slice(&s).expect("de");
        assert_eq!(r, back);
    }
}
