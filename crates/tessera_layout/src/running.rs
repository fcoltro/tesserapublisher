//! What a page says, for the text that reads it.
//!
//! Two things live here. [`OnPage`] is the document's styles with a page's
//! answers attached, so the shaper — which resolves styles through one trait
//! and knows nothing of pages — is handed one object that answers both.
//! [`Running`] is the running headers: for every page, what the first and last
//! paragraph in each named style say, read off the page's own layout after it
//! has been made and before the parent's items that quote it are.

use std::collections::HashMap;

use tessera_document::document::Document;
use tessera_document::ids::PageId;
use tessera_document::nodes::FrameKind;
use tessera_document::variables::{VariableKind, Which};
use tessera_text::story::{
    CharacterFormat, CharacterStyleId, ParagraphFormat, ParagraphStyleId, Styles,
};
use tessera_text::variables::Variables;

use crate::resolve::{ResolvedItem, ResolvedKind};

/// The document's styles, on one page.
///
/// Delegates every style question to the document and answers the one the
/// document cannot — what the markers read as here — itself.
pub struct OnPage<'a> {
    doc: &'a Document,
    variables: Variables,
}

impl<'a> OnPage<'a> {
    pub fn new(doc: &'a Document, variables: Variables) -> Self {
        Self { doc, variables }
    }
}

impl Styles for OnPage<'_> {
    fn character(&self, id: CharacterStyleId) -> Option<&CharacterFormat> {
        self.doc.character(id)
    }

    fn paragraph(&self, id: ParagraphStyleId) -> Option<&ParagraphFormat> {
        self.doc.paragraph(id)
    }

    fn document_default(&self) -> CharacterFormat {
        self.doc.document_default()
    }

    fn character_parent(&self, id: CharacterStyleId) -> Option<CharacterStyleId> {
        self.doc.character_parent(id)
    }

    fn paragraph_parent(&self, id: ParagraphStyleId) -> Option<ParagraphStyleId> {
        self.doc.paragraph_parent(id)
    }

    fn variables(&self) -> Option<&Variables> {
        Some(&self.variables)
    }
}

/// The first and last paragraph in each style, on each page.
#[derive(Debug, Default)]
pub struct Running {
    /// Keyed by page and style; the value is `(first, last)`.
    headers: HashMap<(PageId, ParagraphStyleId), (String, String)>,
}

impl Running {
    /// Read the headers off `items`, the pages' own resolved frames.
    ///
    /// Only the styles some running-header variable names are read, so a
    /// document with no such variable pays nothing here. A paragraph counts on
    /// a page if any line of it is laid out there, in paint order of frames
    /// and line order within a frame — which is reading order for the
    /// ordinary case of one body frame and the least surprising answer for
    /// the rest.
    pub fn read(doc: &Document, items: &[ResolvedItem]) -> Self {
        let wanted: Vec<ParagraphStyleId> = doc
            .variables
            .iter()
            .filter_map(|v| match &v.kind {
                VariableKind::RunningHeader { style, .. } => Some(*style),
                VariableKind::Custom(_) => None,
            })
            .collect();
        let mut headers: HashMap<(PageId, ParagraphStyleId), (String, String)> = HashMap::new();
        if wanted.is_empty() {
            return Self { headers };
        }

        for item in items {
            let (Some(on), ResolvedKind::Text { shaped, .. }) = (item.on, &item.kind) else {
                continue;
            };
            let Some(FrameKind::Text { story, .. }) = doc.frame(item.frame).map(|f| &f.kind) else {
                continue;
            };
            let Some(story) = doc.story(*story) else {
                continue;
            };
            // Each paragraph once, however many of its lines are here.
            let mut seen: Vec<usize> = Vec::new();
            for line in &shaped.lines {
                for para in &story.paragraphs {
                    if para.range.start >= line.range.end || para.range.end <= line.range.start {
                        continue;
                    }
                    let Some(style) = para.style.filter(|s| wanted.contains(s)) else {
                        continue;
                    };
                    if seen.contains(&para.range.start) {
                        continue;
                    }
                    seen.push(para.range.start);
                    let text =
                        tessera_text::variables::expand(&story.text[para.range.clone()], None);
                    let text = text.trim_end_matches('\n').to_owned();
                    let slot = headers
                        .entry((on, style))
                        .or_insert_with(|| (text.clone(), text.clone()));
                    slot.1 = text;
                }
            }
        }
        Self { headers }
    }

    /// What the running header in `style` says on `page`, if anything there is
    /// in that style.
    pub fn header(&self, page: PageId, style: ParagraphStyleId, which: Which) -> Option<String> {
        self.headers
            .get(&(page, style))
            .map(|(first, last)| match which {
                Which::First => first.clone(),
                Which::Last => last.clone(),
            })
    }
}
