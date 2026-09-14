//! The zip-of-XML both formats are, and the handful of readings both need.

use std::io::{Cursor, Read};
use std::path::Path;

use crate::ImportError;

/// A package opened once, its entries read on demand.
pub(crate) struct Package {
    zip: zip::ZipArchive<Cursor<Vec<u8>>>,
}

impl Package {
    pub(crate) fn open(path: &Path, kind: &'static str) -> Result<Self, ImportError> {
        let bytes = std::fs::read(path).map_err(|_| ImportError::Read(path.to_path_buf()))?;
        Self::from_bytes(bytes, path, kind)
    }

    pub(crate) fn from_bytes(
        bytes: Vec<u8>,
        path: &Path,
        kind: &'static str,
    ) -> Result<Self, ImportError> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ImportError::NotAPackage(path.to_path_buf(), kind, e.to_string()))?;
        Ok(Self { zip })
    }

    /// The text of one entry. Entry names are matched exactly, then with
    /// either slash, because packages written on Windows have been seen with
    /// backslashes in them.
    pub(crate) fn text(&mut self, entry: &str) -> Result<String, ImportError> {
        let name = self
            .find(entry)
            .ok_or_else(|| ImportError::Missing(entry.to_owned()))?;
        let mut out = String::new();
        self.zip
            .by_name(&name)
            .map_err(|_| ImportError::Missing(entry.to_owned()))?
            .read_to_string(&mut out)
            .map_err(|e| ImportError::Parse {
                entry: entry.to_owned(),
                message: e.to_string(),
            })?;
        Ok(out)
    }

    pub(crate) fn has(&mut self, entry: &str) -> bool {
        self.find(entry).is_some()
    }

    fn find(&mut self, entry: &str) -> Option<String> {
        let wanted = entry.replace('\\', "/");
        self.zip
            .file_names()
            .find(|n| n.replace('\\', "/") == wanted)
            .map(str::to_owned)
    }
}

/// Parse one entry's XML, naming the entry in any error.
pub(crate) fn parse<'a>(
    entry: &str,
    text: &'a str,
) -> Result<roxmltree::Document<'a>, ImportError> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|e| ImportError::Parse {
        entry: entry.to_owned(),
        message: e.to_string(),
    })
}

/// A node's attribute, by local name, whatever prefix it carries.
pub(crate) fn attr<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

pub(crate) fn attr_f64(node: roxmltree::Node, name: &str) -> Option<f64> {
    attr(node, name).and_then(|v| v.trim().parse().ok())
}

pub(crate) fn attr_f32(node: roxmltree::Node, name: &str) -> Option<f32> {
    attr(node, name).and_then(|v| v.trim().parse().ok())
}

/// Whitespace-separated numbers: "1 0 0 1 -612 -396".
pub(crate) fn numbers(text: &str) -> Vec<f64> {
    text.split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// The first child element with this local name.
pub(crate) fn child<'a, 'i>(
    node: roxmltree::Node<'a, 'i>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'i>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

/// Every child element with this local name.
pub(crate) fn children<'a, 'i>(
    node: roxmltree::Node<'a, 'i>,
    name: &'i str,
) -> impl Iterator<Item = roxmltree::Node<'a, 'i>> + 'i
where
    'a: 'i,
{
    node.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name)
}

/// A typed property in IDML's `<Properties>` block: `<AppliedFont
/// type="string">Minion Pro</AppliedFont>`.
pub(crate) fn property<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    let properties = child(node, "Properties")?;
    child(properties, name).and_then(|n| n.text())
}
