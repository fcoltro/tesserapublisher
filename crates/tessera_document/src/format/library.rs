//! Libraries and snippets: objects kept to place again, in any document.
//!
//! **A snippet is a small document.** The objects are copied out of the
//! document they were in with everything they need — their stories, styles,
//! swatches and links — by the same transfer copy and paste use, into a
//! document of their own; placing one copies them back the same way. So a
//! snippet placed in a document that has no such style brings the style, and
//! one placed where the style is already defined uses that.
//!
//! A snippet file (`.tsnip`) holds one; a library (`.tlib`) holds a list of
//! them, each with its name, as InDesign's library panel does. Both are JSON
//! carrying the format version their documents were written at, so a library
//! made by an older build is brought forward by the same migrations a saved
//! document is.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tessera_geometry::DocRect;

use super::{FORMAT_VERSION, FormatError};
use crate::document::Document;
use crate::ids::FrameId;

/// Objects kept to place again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub name: String,
    /// The objects, top-level in `document`, back to front.
    pub roots: Vec<FrameId>,
    pub document: Document,
}

impl Snippet {
    /// `roots` of `source`, copied into a snippet of their own named `name`.
    pub fn of(
        source: &Document,
        roots: &[FrameId],
        name: impl Into<String>,
    ) -> Result<Snippet, &'static str> {
        let mut document = Document::new();
        let layer = document
            .default_layer()
            .ok_or("a new document has no layer")?;
        // In the order they stand, so they keep it when placed.
        let order = source.top_level_order();
        let mut ordered: Vec<FrameId> = order
            .iter()
            .copied()
            .filter(|f| roots.contains(f))
            .collect();
        for root in roots {
            if !ordered.contains(root) && source.frame(*root).is_some() {
                ordered.push(*root);
            }
        }
        if ordered.is_empty() {
            return Err("nothing to keep");
        }
        let roots = document.import_frames(source, &ordered, layer, 0.0, 0.0, false)?;
        Ok(Snippet {
            name: name.into(),
            roots,
            document,
        })
    }

    /// The box the objects stand in, turned as they are.
    pub fn bounds(&self) -> Option<DocRect> {
        let corners: Vec<_> = self
            .roots
            .iter()
            .filter_map(|r| self.document.frame(*r))
            .flat_map(|f| f.corners())
            .collect();
        if corners.is_empty() {
            return None;
        }
        let (x0, y0, x1, y1) = corners.iter().fold(
            (f64::MAX, f64::MAX, f64::MIN, f64::MIN),
            |(x0, y0, x1, y1), p| (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)),
        );
        Some(DocRect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    /// Copy the objects into `target` on `layer`, their box centred on
    /// `centre`. Returns the new objects.
    pub fn place(
        &self,
        target: &mut Document,
        layer: crate::ids::LayerId,
        centre: tessera_geometry::DocPoint,
    ) -> Result<Vec<FrameId>, &'static str> {
        let b = self.bounds().ok_or("the snippet is empty")?;
        let dx = centre.x - (b.x + b.width / 2.0);
        let dy = centre.y - (b.y + b.height / 2.0);
        target.import_frames(&self.document, &self.roots, layer, dx, dy, false)
    }
}

/// A list of snippets, kept in a file.
#[derive(Debug, Clone, Default)]
pub struct Library {
    pub items: Vec<Snippet>,
}

/// What a library or snippet file holds on disk.
#[derive(Serialize, Deserialize)]
struct OnDisk {
    /// "library" or "snippet", so one is not opened as the other.
    kind: String,
    format_version: u32,
    items: Vec<serde_json::Value>,
}

fn write(kind: &str, items: &[Snippet], path: &Path) -> Result<(), FormatError> {
    let items = items
        .iter()
        .map(|s| serde_json::to_value(s).map_err(|e| FormatError::Write(e.to_string())))
        .collect::<Result<Vec<_>, _>>()?;
    let disk = OnDisk {
        kind: kind.to_owned(),
        format_version: FORMAT_VERSION,
        items,
    };
    let bytes = serde_json::to_vec(&disk).map_err(|e| FormatError::Write(e.to_string()))?;
    tessera_io::atomic::write_atomic(path, &bytes)?;
    Ok(())
}

fn read(kind: &'static str, path: &Path) -> Result<Vec<Snippet>, FormatError> {
    let bytes = std::fs::read(path).map_err(|_| FormatError::Read(path.to_path_buf()))?;
    let disk: OnDisk = serde_json::from_slice(&bytes).map_err(|source| FormatError::Parse {
        entry: kind,
        source,
    })?;
    if disk.kind != kind {
        return Err(FormatError::Archive(format!(
            "this is a {} file, not a {kind}",
            disk.kind
        )));
    }
    if disk.format_version > FORMAT_VERSION {
        return Err(FormatError::NewerFormat {
            found: disk.format_version,
            supported: FORMAT_VERSION,
        });
    }
    disk.items
        .into_iter()
        .map(|mut item| {
            if let Some(document) = item.get_mut("document") {
                super::migrate(document, disk.format_version);
            }
            serde_json::from_value(item).map_err(|source| FormatError::Parse {
                entry: kind,
                source,
            })
        })
        .collect()
}

impl Library {
    pub fn save(&self, path: &Path) -> Result<(), FormatError> {
        write("library", &self.items, path)
    }

    pub fn load(path: &Path) -> Result<Library, FormatError> {
        Ok(Library {
            items: read("library", path)?,
        })
    }
}

/// Write one snippet to its own file.
pub fn save_snippet(snippet: &Snippet, path: &Path) -> Result<(), FormatError> {
    write("snippet", std::slice::from_ref(snippet), path)
}

/// Read a snippet file.
pub fn load_snippet(path: &Path) -> Result<Snippet, FormatError> {
    read("snippet", path)?
        .into_iter()
        .next()
        .ok_or(FormatError::MissingEntry("snippet"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::{Frame, FrameKind};
    use tessera_geometry::{DocPoint, DocRect};

    fn rect(x: f64, y: f64) -> DocRect {
        DocRect {
            x,
            y,
            width: 40.0,
            height: 20.0,
        }
    }

    fn frame_at(bounds: DocRect) -> Frame {
        Frame {
            bounds,
            kind: FrameKind::Rectangle,
            transform: tessera_geometry::Transform::IDENTITY,
            fill: crate::paint::Paint::Solid(tessera_color::Color::BLACK),
            stroke: None,
            wrap: crate::nodes::TextWrap::None,
            blend: crate::blending::Blending::PLAIN,
            corners: crate::corners::Corners::SQUARE,
            shadow: None,
            feather: None,
            anchor: None,
            style: None,
            hidden: false,
            locked: false,
            overprint: Default::default(),
        }
    }

    fn a_document_with_text() -> (Document, FrameId, FrameId) {
        let mut doc = Document::new();
        let layer = doc.default_layer().expect("layer");
        let story = doc.add_story(tessera_text::story::Story::new("kept words"));
        let mut text = frame_at(rect(100.0, 100.0));
        text.kind = FrameKind::text(story);
        let text = doc.add_frame(layer, text);
        let box_ = doc.add_frame(layer, frame_at(rect(160.0, 100.0)));
        (doc, text, box_)
    }

    #[test]
    fn a_snippet_keeps_its_objects_and_their_stories_through_a_file() {
        let (doc, text, box_) = a_document_with_text();
        let snippet = Snippet::of(&doc, &[box_, text], "Header").expect("made");
        assert_eq!(snippet.roots.len(), 2);
        let path = std::env::temp_dir().join(format!("tessera-{}.tsnip", std::process::id()));
        save_snippet(&snippet, &path).expect("saved");
        let back = load_snippet(&path).expect("read");
        let _ = std::fs::remove_file(&path);
        assert_eq!(back.name, "Header");
        assert_eq!(back.bounds(), snippet.bounds());
        let words: Vec<String> = back
            .roots
            .iter()
            .filter_map(|r| back.document.frame(*r))
            .filter_map(|f| match &f.kind {
                FrameKind::Text { story, .. } => {
                    back.document.story(*story).map(|s| s.text.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(words, vec!["kept words".to_string()]);
    }

    #[test]
    fn a_placed_snippet_is_centred_where_it_is_put() {
        let (doc, text, box_) = a_document_with_text();
        let snippet = Snippet::of(&doc, &[text, box_], "Pair").expect("made");
        let mut into = Document::new();
        let layer = into.default_layer().expect("layer");
        let placed = snippet
            .place(&mut into, layer, DocPoint { x: 300.0, y: 300.0 })
            .expect("placed");
        assert_eq!(placed.len(), 2);
        let again = Snippet::of(&into, &placed, "x").expect("again");
        let b = again.bounds().expect("bounds");
        assert!((b.x + b.width / 2.0 - 300.0).abs() < 1e-9);
        assert!((b.y + b.height / 2.0 - 300.0).abs() < 1e-9);
    }

    #[test]
    fn a_library_keeps_its_snippets_in_order_and_will_not_open_as_a_snippet() {
        let (doc, text, box_) = a_document_with_text();
        let library = Library {
            items: vec![
                Snippet::of(&doc, &[text], "One").expect("one"),
                Snippet::of(&doc, &[box_], "Two").expect("two"),
            ],
        };
        let path = std::env::temp_dir().join(format!("tessera-{}.tlib", std::process::id()));
        library.save(&path).expect("saved");
        let back = Library::load(&path).expect("read");
        let names: Vec<&str> = back.items.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["One", "Two"]);
        assert!(load_snippet(&path).is_err(), "a library is not a snippet");
        let _ = std::fs::remove_file(&path);
    }
}
