//! A book's styles kept the same across its chapters: InDesign's
//! Synchronize.
//!
//! One chapter is the style source. Every swatch and every paragraph,
//! character, object, table and cell style it has is made the same in
//! another chapter **by name**: a style of that name is given the source's
//! definition, and one the chapter lacks is added. Nothing the chapter has
//! of its own is taken away — a chapter's extra styles are its business —
//! and nothing is applied to any text: a paragraph already set in "Body"
//! takes the new "Body" because it names it.
//!
//! A style names others — the style it is based on, a table style's cell
//! styles — by id, and ids are a document's own. Each is translated through
//! the names, so "based on Body" in the source is "based on this chapter's
//! Body" here.

use std::collections::HashMap;
use std::hash::Hash;

use serde::{Deserialize, Serialize};
use slotmap::{Key, SlotMap};
use tessera_text::story::{CharacterStyle, CharacterStyleId, ParagraphStyle, ParagraphStyleId};

use crate::document::Document;
use crate::ids::{CellStyleId, ObjectStyleId, TableStyleId};
use crate::nodes::Swatch;
use crate::object_style::ObjectStyle;
use crate::table_style::{CellStyle, Stated, TableStyle};

/// A document's swatches and styles, taken out of it: what a style source
/// hands a chapter. Each style with the id it has in its own document, so
/// the ids its styles name can be translated.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleSheet {
    pub swatches: Vec<Swatch>,
    pub character: Vec<(CharacterStyleId, CharacterStyle)>,
    pub paragraph: Vec<(ParagraphStyleId, ParagraphStyle)>,
    /// In the order the Object Styles panel lists them.
    pub object: Vec<(ObjectStyleId, ObjectStyle)>,
    pub cell: Vec<(CellStyleId, CellStyle)>,
    pub table: Vec<(TableStyleId, TableStyle)>,
}

/// A slot map's entries as pairs, in its own order.
fn pairs<K: Key, V: Clone>(map: &SlotMap<K, V>) -> Vec<(K, V)> {
    map.iter().map(|(k, v)| (k, v.clone())).collect()
}

impl StyleSheet {
    pub fn of(doc: &Document) -> Self {
        let mut object: Vec<(ObjectStyleId, ObjectStyle)> = doc
            .object_style_order
            .iter()
            .filter_map(|id| Some((*id, doc.object_styles.get(*id)?.clone())))
            .collect();
        for (id, style) in &doc.object_styles {
            if !doc.object_style_order.contains(&id) {
                object.push((id, style.clone()));
            }
        }
        Self {
            swatches: doc.swatches.clone(),
            character: pairs(&doc.character_styles),
            paragraph: pairs(&doc.paragraph_styles),
            object,
            cell: pairs(&doc.cell_styles),
            table: pairs(&doc.table_styles),
        }
    }
}

/// How many swatches and styles a synchronisation added, and how many it
/// changed. Those already the same are neither.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Synced {
    pub added: usize,
    pub changed: usize,
}

impl Synced {
    pub fn is_empty(&self) -> bool {
        self.added == 0 && self.changed == 0
    }
}

/// Every source style's id in the target: the target's style of the same
/// name, or a new one made for it. New ones are placeholders here; their
/// definitions are written once every id is known.
fn match_by_name<K: Key + Hash, V: Clone>(
    source: &[(K, V)],
    target: &mut SlotMap<K, V>,
    name: impl Fn(&V) -> &str,
) -> (HashMap<K, K>, Vec<K>) {
    let mut ids = HashMap::new();
    let mut added = Vec::new();
    for (id, style) in source {
        let existing = target
            .iter()
            .find(|(_, t)| name(t) == name(style))
            .map(|(k, _)| k);
        let mapped = existing.unwrap_or_else(|| {
            let k = target.insert(style.clone());
            added.push(k);
            k
        });
        ids.insert(*id, mapped);
    }
    (ids, added)
}

/// Write the source's definition over the target's, counting it as changed
/// when it was there and differs.
fn write<K: Key + Hash, V: Clone + PartialEq>(
    target: &mut SlotMap<K, V>,
    at: K,
    style: V,
    added: &[K],
    synced: &mut Synced,
) {
    let Some(slot) = target.get_mut(at) else {
        return;
    };
    if added.contains(&at) {
        synced.added += 1;
    } else if *slot != style {
        synced.changed += 1;
    }
    *slot = style;
}

impl Document {
    /// Make this document's swatches and styles the same as `source`'s,
    /// by name. See the module's note.
    pub fn synchronise_styles(&mut self, source: &StyleSheet) -> Synced {
        let mut synced = Synced::default();

        // Swatches first: a style's colour names one.
        for swatch in &source.swatches {
            match self.swatches.iter_mut().find(|s| s.name == swatch.name) {
                Some(mine) if mine != swatch => {
                    *mine = swatch.clone();
                    synced.changed += 1;
                }
                Some(_) => {}
                None => {
                    self.swatches.push(swatch.clone());
                    synced.added += 1;
                }
            }
        }

        let (characters, added) =
            match_by_name(&source.character, &mut self.character_styles, |s| &s.name);
        for (id, style) in &source.character {
            let mut style = style.clone();
            style.based_on = style.based_on.and_then(|b| characters.get(&b).copied());
            write(
                &mut self.character_styles,
                characters[id],
                style,
                &added,
                &mut synced,
            );
        }

        let (paragraphs, added) =
            match_by_name(&source.paragraph, &mut self.paragraph_styles, |s| &s.name);
        for (id, style) in &source.paragraph {
            let mut style = style.clone();
            style.based_on = style.based_on.and_then(|b| paragraphs.get(&b).copied());
            write(
                &mut self.paragraph_styles,
                paragraphs[id],
                style,
                &added,
                &mut synced,
            );
        }

        let (objects, added) = match_by_name(&source.object, &mut self.object_styles, |s| &s.name);
        // New object styles join the list the panel shows, in the source's
        // order.
        for (id, _) in &source.object {
            if let Some(mapped) = objects.get(id)
                && added.contains(mapped)
            {
                self.object_style_order.push(*mapped);
            }
        }
        for (id, style) in &source.object {
            let mut style = style.clone();
            style.based_on = style.based_on.and_then(|b| objects.get(&b).copied());
            write(
                &mut self.object_styles,
                objects[id],
                style,
                &added,
                &mut synced,
            );
        }

        let (cells, added) = match_by_name(&source.cell, &mut self.cell_styles, |s| &s.name);
        for (id, style) in &source.cell {
            let mut style = style.clone();
            style.based_on = style.based_on.and_then(|b| cells.get(&b).copied());
            // The text's paragraph style, as this document numbers it.
            if let Stated::Is(Some(p)) = style.format.paragraph {
                style.format.paragraph = Stated::Is(paragraphs.get(&p).copied());
            }
            write(&mut self.cell_styles, cells[id], style, &added, &mut synced);
        }

        let (tables, added) = match_by_name(&source.table, &mut self.table_styles, |s| &s.name);
        let cell = |stated: &Stated<Option<CellStyleId>>| match stated {
            Stated::Is(Some(id)) => Stated::Is(cells.get(id).copied()),
            other => other.clone(),
        };
        for (id, style) in &source.table {
            let mut style = style.clone();
            style.based_on = style.based_on.and_then(|b| tables.get(&b).copied());
            style.format.header = cell(&style.format.header);
            style.format.body = cell(&style.format.body);
            style.format.footer = cell(&style.format.footer);
            write(
                &mut self.table_styles,
                tables[id],
                style,
                &added,
                &mut synced,
            );
        }

        if !synced.is_empty() {
            self.touch();
        }
        synced
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_color::Color;
    use tessera_text::story::{CharacterFormat, ParagraphFormat, ParagraphStyle};

    fn paragraph(name: &str, size: f32) -> ParagraphStyle {
        ParagraphStyle {
            name: name.into(),
            format: ParagraphFormat {
                character: CharacterFormat {
                    size: Some(size),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn a_chapter_takes_the_source_styles_by_name_and_keeps_its_own() {
        let mut source = Document::new();
        let body = source.paragraph_styles.insert(paragraph("Body", 10.0));
        let mut quote = paragraph("Quote", 9.0);
        quote.based_on = Some(body);
        source.paragraph_styles.insert(quote);
        source
            .swatches
            .push(crate::nodes::Swatch::new("Brand", Color::BLACK_INK));

        let mut chapter = Document::new();
        let old_body = chapter.paragraph_styles.insert(paragraph("Body", 12.0));
        chapter.paragraph_styles.insert(paragraph("Aside", 8.0));

        let synced = chapter.synchronise_styles(&StyleSheet::of(&source));
        assert_eq!(synced.changed, 1, "Body, redefined");
        assert!(synced.added >= 2, "Quote and the swatch: {synced:?}");

        let named = |doc: &Document, name: &str| {
            doc.paragraph_styles
                .iter()
                .find(|(_, s)| s.name == name)
                .map(|(id, s)| (id, s.clone()))
        };
        let (body_id, body) = named(&chapter, "Body").expect("Body");
        assert_eq!(
            body_id, old_body,
            "the same style, so text set in it follows"
        );
        assert_eq!(body.format.character.size, Some(10.0));
        let (_, quote) = named(&chapter, "Quote").expect("Quote added");
        assert_eq!(
            quote.based_on,
            Some(old_body),
            "based on this chapter's Body"
        );
        assert!(named(&chapter, "Aside").is_some(), "its own style is kept");
        assert!(chapter.swatch("Brand").is_some());

        // Again: nothing left to do.
        assert!(
            chapter
                .synchronise_styles(&StyleSheet::of(&source))
                .is_empty()
        );
    }
}
