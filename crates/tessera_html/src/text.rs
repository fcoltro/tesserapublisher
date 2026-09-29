//! Stories as HTML: paragraphs, runs, and the markers set in them.

use std::collections::HashMap;
use std::ops::Range;

use tessera_document::document::Document;
use tessera_document::ids::{FrameId, StoryId};
use tessera_text::story::{Hyperlink, ListKind, ParagraphStyleId, Story, Styles};
use tessera_text::variables::Marker;

use crate::css::{self, Classes};
use crate::order::Block;

/// `<`, `>`, `&` and `"` as HTML writes them.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// The document's language as HTML names it: "en-GB" for `en_GB`, English
/// when the document says none.
pub fn document_language(doc: &Document) -> String {
    doc.document_default()
        .language
        .filter(|l| !l.trim().is_empty())
        .map_or_else(|| "en".to_owned(), |l| l.replace('_', "-"))
}

/// The id a text anchor goes out with, for a cross-reference or a link to
/// point at.
pub fn anchor_id(name: &str) -> String {
    format!("a-{}", css::slug(name))
}

/// Writes blocks one after another, numbering footnotes through the whole
/// document as the notes at the foot of a web page are.
pub struct Writer<'a> {
    doc: &'a Document,
    classes: &'a Classes,
    /// The paragraph styles the contents list, by level: set as headings.
    headings: HashMap<ParagraphStyleId, usize>,
    notes_written: usize,
    files: Vec<(String, Vec<u8>)>,
    /// Every heading written: its level, its id and its words as HTML —
    /// what an EPUB's table of contents lists.
    pub written_headings: Vec<(usize, String, String)>,
    ppi: f64,
    images: crate::ImageFormat,
}

impl<'a> Writer<'a> {
    pub fn new(doc: &'a Document, classes: &'a Classes, options: &crate::Options) -> Self {
        let headings = doc
            .contents
            .levels
            .iter()
            .enumerate()
            .map(|(level, l)| (l.style, level + 1))
            .collect();
        Self {
            doc,
            classes,
            headings,
            notes_written: 0,
            files: Vec::new(),
            written_headings: Vec::new(),
            ppi: options.ppi,
            images: options.images,
        }
    }

    /// A picture's `<img>`, its file written beside the page; nothing for a
    /// frame showing nothing or a file that will not read.
    fn picture(&mut self, frame: FrameId) -> Option<String> {
        let picture = crate::pictures::picture(self.doc, frame, self.ppi, self.images)?;
        let name = format!(
            "{}/{}-{}.{}",
            crate::IMAGES,
            css::slug(&picture.stem),
            self.files.len() + 1,
            picture.extension
        );
        let tag = format!(
            "<img src=\"{}\" alt=\"{}\" width=\"{}\" height=\"{}\" />",
            escape(&name),
            escape(&picture.stem),
            picture.width.round(),
            picture.height.round()
        );
        self.files.push((name, picture.bytes));
        Some(tag)
    }

    pub fn into_files(self) -> Vec<(String, Vec<u8>)> {
        self.files
    }

    pub fn block(&mut self, block: &Block, out: &mut String) {
        match *block {
            Block::Story(id) => self.story(id, out),
            Block::Picture(frame) => {
                if let Some(img) = self.picture(frame) {
                    out.push_str(&format!("<figure class=\"picture\">{img}</figure>\n"));
                }
            }
            Block::Table(frame) => self.table(frame, out),
        }
    }

    /// A story, and after it the notes it cites.
    pub fn story(&mut self, id: StoryId, out: &mut String) {
        let Some(story) = self.doc.story(id) else {
            return;
        };
        let mut notes: Vec<(usize, &Story)> = Vec::new();
        self.paragraphs(story, Some(id), &mut notes, None, out);
        self.notes(notes, out);
    }

    /// A table: its heading rows in a `thead`, its footing rows in a
    /// `tfoot`, spans as spans, the columns' widths, and each cell's fill,
    /// edges, insets and vertical placement as its style — as the table's
    /// and cells' styles make them. The notes its cells cite follow it.
    pub fn table(&mut self, frame: FrameId, out: &mut String) {
        use tessera_document::nodes::{FrameKind, VerticalJustify};
        use tessera_document::table::Slot;
        let doc = self.doc;
        let Some(FrameKind::Table(table)) = doc.frame(frame).map(|f| &f.kind) else {
            return;
        };
        let table = doc.styled_table(table);
        let (rows, columns) = (table.rows(), table.columns());
        let header = usize::from(table.header_rows).min(rows);
        let footer = usize::from(table.footer_rows).min(rows - header);
        let body = rows - header - footer;
        let stroke = |s: &tessera_document::nodes::Stroke| {
            format!(
                "{}pt solid {}",
                css::number(s.width as f32),
                css::colour(doc, &s.color)
            )
        };
        let paint =
            |p: &tessera_document::paint::Paint| p.colours().first().map(|c| css::colour(doc, c));

        out.push_str("<table class=\"table\">\n<colgroup>");
        for width in &table.columns {
            out.push_str(&format!(
                "<col style=\"width: {}pt\" />",
                css::number(*width as f32)
            ));
        }
        out.push_str("</colgroup>\n");
        let mut notes: Vec<(usize, &Story)> = Vec::new();
        for row in 0..rows {
            let (section, cell_tag) = if row < header {
                ("thead", "th")
            } else if row >= rows - footer {
                ("tfoot", "td")
            } else {
                ("tbody", "td")
            };
            let first_of_section = row == 0 || row == header || row == rows - footer;
            if first_of_section {
                if row > 0 {
                    let before = if row == header { "thead" } else { "tbody" };
                    out.push_str(&format!("</{before}>\n"));
                }
                out.push_str(&format!("<{section}>\n"));
            }
            out.push_str("<tr>");
            for column in 0..columns {
                let Some(Slot::Cell(cell)) = table.at(row, column) else {
                    continue;
                };
                let mut style = Vec::new();
                // A body row's turn in the alternating fills, under the
                // cell's own.
                let fill = cell.fill.as_ref().and_then(paint).or_else(|| {
                    (row >= header && row < header + body)
                        .then(|| table.alternating.as_ref())
                        .flatten()
                        .and_then(|a| a.fill_for_row(row - header, body))
                        .and_then(paint)
                });
                if let Some(fill) = fill {
                    style.push(format!("background: {fill}"));
                }
                for (side, edge) in [
                    ("top", &cell.edges.top),
                    ("right", &cell.edges.right),
                    ("bottom", &cell.edges.bottom),
                    ("left", &cell.edges.left),
                ] {
                    if let Some(s) = edge.as_ref().or(table.stroke.as_ref()) {
                        style.push(format!("border-{side}: {}", stroke(s)));
                    }
                }
                let inset = cell.inset;
                style.push(format!(
                    "padding: {}pt {}pt {}pt {}pt",
                    css::number(inset.top as f32),
                    css::number(inset.right as f32),
                    css::number(inset.bottom as f32),
                    css::number(inset.left as f32)
                ));
                style.push(
                    match cell.vertical {
                        VerticalJustify::Top | VerticalJustify::Justify => "vertical-align: top",
                        VerticalJustify::Centre => "vertical-align: middle",
                        VerticalJustify::Bottom => "vertical-align: bottom",
                    }
                    .to_owned(),
                );
                out.push_str(&format!("<{cell_tag}"));
                if cell.span.columns > 1 {
                    out.push_str(&format!(" colspan=\"{}\"", cell.span.columns));
                }
                if cell.span.rows > 1 {
                    out.push_str(&format!(" rowspan=\"{}\"", cell.span.rows));
                }
                out.push_str(&format!(" style=\"{}\">", escape(&style.join("; "))));
                if let Some(story) = doc.story(cell.story) {
                    let mut inner = String::new();
                    self.paragraphs(story, Some(cell.story), &mut notes, None, &mut inner);
                    out.push_str(inner.trim_end());
                }
                out.push_str(&format!("</{cell_tag}>"));
            }
            out.push_str("</tr>\n");
        }
        if rows > 0 {
            let last = if footer > 0 {
                "tfoot"
            } else if body > 0 {
                "tbody"
            } else {
                "thead"
            };
            out.push_str(&format!("</{last}>\n"));
        }
        out.push_str("</table>\n");
        self.notes(notes, out);
    }

    /// The notes a story or a table cites, numbered as they were cited.
    fn notes(&mut self, notes: Vec<(usize, &Story)>, out: &mut String) {
        if notes.is_empty() {
            return;
        }
        out.push_str("<aside class=\"footnotes\">\n<ol>\n");
        for (number, note) in notes {
            out.push_str(&format!("<li id=\"fn-{number}\" value=\"{number}\">"));
            let mut inner = String::new();
            self.paragraphs(note, None, &mut Vec::new(), Some(number), &mut inner);
            out.push_str(inner.trim_end());
            out.push_str(&format!(
                " <a href=\"#fnref-{number}\" class=\"footnote-back\">\u{21a9}</a></li>\n"
            ));
        }
        out.push_str("</ol>\n</aside>\n");
    }

    /// Every paragraph of `story`: a heading when its style is one the
    /// contents list, an item of a list when it is one, a paragraph else.
    fn paragraphs<'s>(
        &mut self,
        story: &'s Story,
        id: Option<StoryId>,
        notes: &mut Vec<(usize, &'s Story)>,
        note_number: Option<usize>,
        out: &mut String,
    ) {
        let mut list: Option<ListKind> = None;
        for range in story.paragraph_ranges() {
            let run = story.paragraph_run_at(range.start);
            let style = run.and_then(|r| r.style);
            let local = run.map(|r| r.local.clone()).unwrap_or_default();
            let chain = style
                .map(|s| self.doc.paragraph_chain(s))
                .unwrap_or_default();
            let kind = local
                .list
                .as_ref()
                .or(chain.list.as_ref())
                .map(|l| l.kind)
                .filter(|k| *k != ListKind::None);
            if kind != list {
                close_list(list, out);
                match kind {
                    Some(ListKind::Bullet) => out.push_str("<ul>\n"),
                    Some(ListKind::Number) => out.push_str("<ol>\n"),
                    _ => {}
                }
                list = kind;
            }
            let tag = match (kind, style.and_then(|s| self.headings.get(&s))) {
                (Some(_), _) => "li".to_owned(),
                (None, Some(level)) => format!("h{}", (*level).min(6)),
                (None, None) => "p".to_owned(),
            };
            let mut declarations = css::paragraph_declarations(self.doc, &local);
            declarations.extend(css::declarations(self.doc, &local.character));
            let heading = (tag.starts_with('h') && note_number.is_none())
                .then(|| tag[1..].parse::<usize>().ok())
                .flatten();
            out.push('<');
            out.push_str(&tag);
            let heading_id = heading.map(|_| format!("h-{}", self.written_headings.len() + 1));
            if let Some(id) = &heading_id {
                out.push_str(&format!(" id=\"{id}\""));
            }
            if let Some(class) = style.and_then(|s| self.classes.paragraph.get(&s)) {
                out.push_str(&format!(" class=\"{class}\""));
            }
            if !declarations.is_empty() {
                out.push_str(&format!(" style=\"{}\"", escape(&declarations.join("; "))));
            }
            out.push('>');
            let end = if story.text[range.clone()].ends_with('\n') {
                range.end - 1
            } else {
                range.end
            };
            let mut tables = Vec::new();
            let words_from = out.len();
            self.inline(
                story,
                id,
                range.start..end,
                notes,
                note_number,
                &mut tables,
                out,
            );
            if let (Some(level), Some(id)) = (heading, heading_id) {
                let words = strip_tags(&out[words_from..]);
                self.written_headings.push((level, id, words));
            }
            out.push_str(&format!("</{tag}>\n"));
            // A table anchored in the paragraph follows it: a table cannot
            // stand inside a paragraph.
            for frame in tables {
                close_list(list.take(), out);
                self.table(frame, out);
            }
        }
        close_list(list, out);
    }

    /// The runs of `story` over `range`, each in a span when it has a style
    /// or formatting of its own, in a link when it is one.
    #[allow(clippy::too_many_arguments)]
    fn inline<'s>(
        &mut self,
        story: &'s Story,
        id: Option<StoryId>,
        range: Range<usize>,
        notes: &mut Vec<(usize, &'s Story)>,
        note_number: Option<usize>,
        tables: &mut Vec<FrameId>,
        out: &mut String,
    ) {
        let footnotes = story.footnote_offsets();
        // The objects anchored in the text, by their markers' order.
        let anchored = id.map(|id| self.doc.anchors_in(id));
        let markers: Vec<usize> = story
            .text
            .char_indices()
            .filter(|(_, c)| *c == tessera_document::anchored::MARKER)
            .map(|(at, _)| at)
            .collect();
        let anchors = story.anchor_offsets();
        let references = story.cross_reference_offsets();
        for run in &story.runs {
            let start = run.range.start.max(range.start);
            let end = run.range.end.min(range.end);
            if start >= end {
                continue;
            }
            let link = story.resolve_run(run, self.doc).link;
            let href = match &link {
                Some(Hyperlink::Url(url)) if !url.trim().is_empty() => Some(url.trim().to_owned()),
                // A named destination that is a text anchor; a page has no
                // place in a web page to go to.
                Some(Hyperlink::Destination(name)) if anchor_exists(self.doc, name) => {
                    Some(format!("#{}", anchor_id(name)))
                }
                _ => None,
            };
            if let Some(href) = &href {
                out.push_str(&format!("<a href=\"{}\">", escape(href)));
            }
            let class = run.style.and_then(|s| self.classes.character.get(&s));
            let declarations = css::declarations(self.doc, &run.local);
            let span = class.is_some() || !declarations.is_empty();
            if span {
                out.push_str("<span");
                if let Some(class) = class {
                    out.push_str(&format!(" class=\"{class}\""));
                }
                if !declarations.is_empty() {
                    out.push_str(&format!(" style=\"{}\"", escape(&declarations.join("; "))));
                }
                out.push('>');
            }
            for (offset, c) in story.text[start..end].char_indices() {
                let at = start + offset;
                match Marker::of(c) {
                    Some(Marker::FootnoteReference) => {
                        let Some(index) = footnotes.iter().position(|o| *o == at) else {
                            continue;
                        };
                        let Some(note) = story.footnotes.get(index) else {
                            continue;
                        };
                        self.notes_written += 1;
                        let n = self.notes_written;
                        notes.push((n, note));
                        out.push_str(&format!(
                            "<sup class=\"footnote-ref\"><a href=\"#fn-{n}\" id=\"fnref-{n}\">{n}</a></sup>"
                        ));
                    }
                    Some(Marker::FootnoteNumber) => {
                        if let Some(n) = note_number {
                            out.push_str(&n.to_string());
                        }
                    }
                    Some(Marker::TextAnchor) => {
                        if let Some(anchor) = anchors
                            .iter()
                            .position(|o| *o == at)
                            .and_then(|i| story.anchors.get(i))
                            .filter(|a| !a.name.trim().is_empty())
                        {
                            out.push_str(&format!("<a id=\"{}\"></a>", anchor_id(&anchor.name)));
                        }
                    }
                    Some(Marker::CrossReference) => {
                        if let Some(reference) = references
                            .iter()
                            .position(|o| *o == at)
                            .and_then(|i| story.cross_references.get(i))
                        {
                            let reading = anchor_paragraph(self.doc, &reference.target)
                                .unwrap_or_else(|| reference.target.clone());
                            out.push_str(&format!(
                                "<a href=\"#{}\">{}</a>",
                                anchor_id(&reference.target),
                                escape(&reading)
                            ));
                        }
                    }
                    Some(Marker::Variable(n)) => {
                        out.push_str(&escape(&variable_text(self.doc, usize::from(n))));
                    }
                    Some(Marker::Field(n)) => {
                        out.push_str(&escape(&field_text(self.doc, usize::from(n))));
                    }
                    // Page numbers and section markers have no page here;
                    // an index entry reads as nothing anywhere.
                    Some(_) => {}
                    // An object anchored here: a picture goes out where its
                    // marker is, in the line.
                    None if c == tessera_document::anchored::MARKER => {
                        let frame = markers
                            .iter()
                            .position(|m| *m == at)
                            .and_then(|i| anchored.as_ref()?.frame_at(i));
                        let is_table = frame.is_some_and(|f| {
                            matches!(
                                self.doc.frame(f).map(|f| &f.kind),
                                Some(tessera_document::nodes::FrameKind::Table(_))
                            )
                        });
                        if let Some(frame) = frame {
                            if is_table {
                                tables.push(frame);
                            } else if let Some(img) = self.picture(frame) {
                                out.push_str(&img);
                            }
                        }
                    }
                    None if c == '\t' => out.push(' '),
                    None if c == '\u{2028}' => out.push_str("<br />"),
                    None => out.push_str(&escape(&c.to_string())),
                }
            }
            if span {
                out.push_str("</span>");
            }
            if href.is_some() {
                out.push_str("</a>");
            }
        }
    }
}

/// HTML with its tags taken out, its entities left as they are.
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in html.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    out.trim().to_owned()
}

fn close_list(list: Option<ListKind>, out: &mut String) {
    match list {
        Some(ListKind::Bullet) => out.push_str("</ul>\n"),
        Some(ListKind::Number) => out.push_str("</ol>\n"),
        _ => {}
    }
}

/// Whether any story holds a text anchor of this name.
fn anchor_exists(doc: &Document, name: &str) -> bool {
    doc.stories
        .values()
        .any(|s| s.anchors.iter().any(|a| a.name == name))
}

/// The paragraph a text anchor stands in, as words: what a cross-reference
/// to it reads as when there are no pages to name.
fn anchor_paragraph(doc: &Document, name: &str) -> Option<String> {
    for story in doc.stories.values() {
        let Some(index) = story.anchors.iter().position(|a| a.name == name) else {
            continue;
        };
        let at = *story.anchor_offsets().get(index)?;
        // The paragraph holding the marker; the last one only for a marker
        // at the very end, which no range contains. Not "contains or ends
        // at": a marker opening a paragraph is where the one before ends.
        let ranges = story.paragraph_ranges();
        let range = ranges
            .iter()
            .find(|r| r.contains(&at))
            .or_else(|| ranges.last().filter(|r| r.end == at))
            .cloned()?;
        let words: String = story.text[range]
            .chars()
            .filter(|c| Marker::of(*c).is_none() && *c != '\n')
            .collect();
        return Some(words.trim().to_owned());
    }
    None
}

/// What a text variable reads as off the page: its own words, the chapter,
/// the file's name or date. The running header and the last page number
/// belong to pages, and read as nothing.
fn variable_text(doc: &Document, n: usize) -> String {
    use tessera_document::variables::VariableKind;
    let Some(variable) = doc.variables.get(n) else {
        return String::new();
    };
    match &variable.kind {
        VariableKind::Custom(text) => text.clone(),
        VariableKind::ChapterNumber => doc.chapter.label(),
        VariableKind::FileName { folder, extension } => {
            doc.file_facts().file_name(*folder, *extension)
        }
        VariableKind::Date { of, format } => doc
            .file_facts()
            .date(*of)
            .map(|stamp| stamp.format(format))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// A merge field: the record shown, or the field's name in chevrons.
fn field_text(doc: &Document, n: usize) -> String {
    let Some(source) = &doc.data_merge else {
        return String::new();
    };
    match doc.merge_record() {
        Some(values) => values.get(n).cloned().unwrap_or_default(),
        None => source
            .fields
            .get(n)
            .map(|f| format!("\u{ab}{}\u{bb}", f.name))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_characters_are_escaped() {
        assert_eq!(escape("a < b & \"c\""), "a &lt; b &amp; &quot;c&quot;");
    }
}
