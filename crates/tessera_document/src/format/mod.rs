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

use crate::document::Document;

/// Bumped whenever the on-disk shape changes. An older version runs
/// migrations; a newer one is refused rather than guessed at.
pub const FORMAT_VERSION: u32 = 1;

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
    // Older versions migrate here. At format 1 there is nothing to migrate.

    read_json(&mut zip, DOCUMENT_ENTRY)
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
    use super::*;
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
