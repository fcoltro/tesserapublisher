//! A Word document, as text to place.
//!
//! `.docx` is a zip holding `word/document.xml`, with the paragraph and
//! character styles in `word/styles.xml` and the footnotes in
//! `word/footnotes.xml`. What comes out is one [`Story`] and the styles it
//! uses, for the caller to add to a document and put in a frame — Word has
//! no pages worth keeping, only words and the styles on them.
//!
//! Direct formatting on a run (`<w:b/>`, `<w:i/>`, `<w:sz/>`) becomes local
//! formatting; a paragraph's `<w:pStyle>` becomes the paragraph style of that
//! name, created if the document lacks it. Half-points are what Word counts
//! sizes in, and twentieths of a point its spacing; both are converted here
//! and nowhere else.

use std::collections::HashMap;
use std::path::Path;

use roxmltree::Node;
use tessera_text::story::{
    Alignment, CharacterFormat, CharacterStyle, Decoration, IndexEntry, ParagraphFormat,
    ParagraphRun, ParagraphStyle, Run, Story,
};
use tessera_text::variables::Marker;

use crate::xml::{Package, attr, child, parse};
use crate::{Dropped, ImportError};

/// What a Word file yields: a story, and the styles it refers to by name.
///
/// Styles are returned rather than added, because the caller has a document
/// with styles of its own, and a "Heading 1" it already defines should win.
#[derive(Debug, Default)]
pub struct Imported {
    pub story: Story,
    /// Paragraph styles by the name the story's runs use, in definition
    /// order. `Story::paragraphs[n].style` is `None` here; the caller maps
    /// [`Imported::paragraph_style_names`] to ids.
    pub paragraph_styles: Vec<ParagraphStyle>,
    pub character_styles: Vec<CharacterStyle>,
    /// One name per paragraph of the story, `None` for the default.
    pub paragraph_style_names: Vec<Option<String>>,
    /// One name per run of the story.
    pub run_style_names: Vec<Option<String>>,
    pub dropped: Dropped,
}

pub fn import(path: &Path) -> Result<Imported, ImportError> {
    let package = Package::open(path, "Word")?;
    import_package(package)
}

pub fn import_bytes(bytes: Vec<u8>, path: &Path) -> Result<Imported, ImportError> {
    let package = Package::from_bytes(bytes, path, "Word")?;
    import_package(package)
}

fn import_package(mut package: Package) -> Result<Imported, ImportError> {
    let mut out = Imported::default();

    // Styles: id → (shown name, kind, format), so a paragraph's pStyle id
    // can become the name a person sees in Word's gallery.
    let mut styles: HashMap<String, (String, bool, ParagraphFormat, CharacterFormat)> =
        HashMap::new();
    if package.has("word/styles.xml") {
        let text = package.text("word/styles.xml")?;
        let xml = parse("word/styles.xml", &text)?;
        for style in xml.descendants().filter(|n| n.tag_name().name() == "style") {
            let Some(id) = attr(style, "styleId") else {
                continue;
            };
            let is_paragraph = attr(style, "type") == Some("paragraph");
            let name = child(style, "name")
                .and_then(|n| attr(n, "val"))
                .unwrap_or(id)
                .to_owned();
            let name = title_case(&name);
            let paragraph = child(style, "pPr").map(paragraph_props).unwrap_or_default();
            let character = child(style, "rPr").map(run_props).unwrap_or_default();
            styles.insert(id.to_owned(), (name, is_paragraph, paragraph, character));
        }
    }

    // Footnotes, by id, as stories.
    let mut footnotes: HashMap<String, Story> = HashMap::new();
    if package.has("word/footnotes.xml") {
        let text = package.text("word/footnotes.xml")?;
        let xml = parse("word/footnotes.xml", &text)?;
        for note in xml
            .descendants()
            .filter(|n| n.tag_name().name() == "footnote")
        {
            let Some(id) = attr(note, "id") else { continue };
            // Separators and continuation notices carry a type; real notes do not.
            if attr(note, "type").is_some() {
                continue;
            }
            let mut b = Builder::default();
            read_body(note, &styles, &HashMap::new(), &mut b, &mut out.dropped);
            let (mut story, _, _) = b.finish();
            let number = format!("{}\t", Marker::FootnoteNumber.character());
            story.insert_text(0, &number);
            footnotes.insert(id.to_owned(), story);
        }
    }

    let text = package.text("word/document.xml")?;
    let xml = parse("word/document.xml", &text)?;
    let Some(body) = xml.descendants().find(|n| n.tag_name().name() == "body") else {
        return Err(ImportError::Missing("word/document.xml body".into()));
    };
    let mut b = Builder::default();
    read_body(body, &styles, &footnotes, &mut b, &mut out.dropped);

    (out.story, out.run_style_names, out.paragraph_style_names) = b.finish();

    // The styles the story uses, as Tessera styles, in first-use order.
    let mut seen: Vec<String> = Vec::new();
    for name in out.paragraph_style_names.iter().flatten() {
        if seen.contains(name) {
            continue;
        }
        seen.push(name.clone());
        if let Some((_, _, paragraph, character)) =
            styles.values().find(|(n, p, _, _)| n == name && *p)
        {
            let mut format = paragraph.clone();
            format.character = character.clone();
            out.paragraph_styles.push(ParagraphStyle {
                name: name.clone(),
                based_on: None,
                format,
            });
        }
    }
    let mut seen: Vec<String> = Vec::new();
    for name in out.run_style_names.iter().flatten() {
        if seen.contains(name) {
            continue;
        }
        seen.push(name.clone());
        if let Some((_, _, _, character)) = styles.values().find(|(n, p, _, _)| n == name && !*p) {
            out.character_styles.push(CharacterStyle {
                name: name.clone(),
                based_on: None,
                format: character.clone(),
            });
        }
    }
    Ok(out)
}

/// "heading 1" → "Heading 1", as Word's gallery shows it.
fn title_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut start = true;
    for c in name.chars() {
        if start {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
        start = c == ' ';
    }
    out
}

#[derive(Default)]
struct Builder {
    text: String,
    runs: Vec<(usize, usize, CharacterFormat)>,
    run_style_names: Vec<Option<String>>,
    paragraphs: Vec<(usize, usize, ParagraphFormat)>,
    paragraph_style_names: Vec<Option<String>>,
    footnotes: Vec<Story>,
}

type FinishedStory = (Story, Vec<Option<String>>, Vec<Option<String>>);

impl Builder {
    fn finish(self) -> FinishedStory {
        let text = self.text;
        if text.is_empty() {
            return (Story::default(), Vec::new(), Vec::new());
        }
        let mut runs = Vec::new();
        let mut run_names = Vec::new();
        let mut at = 0usize;
        for ((start, end, local), name) in self.runs.into_iter().zip(self.run_style_names) {
            let start = start.max(at).min(text.len());
            let end = end.min(text.len());
            if start > at {
                runs.push(Run::plain(at..start));
                run_names.push(None);
            }
            if end > start {
                runs.push(Run {
                    range: start..end,
                    style: None,
                    local,
                });
                run_names.push(name);
                at = end;
            }
        }
        if at < text.len() {
            runs.push(Run::plain(at..text.len()));
            run_names.push(None);
        }
        let mut paragraphs = Vec::new();
        let mut paragraph_names = Vec::new();
        let mut start = 0usize;
        for piece in text.split_inclusive('\n') {
            let source = self
                .paragraphs
                .iter()
                .enumerate()
                .find(|(_, (s, e, _))| *s <= start && start < *e);
            let local = source.map(|(_, (_, _, f))| f.clone()).unwrap_or_default();
            paragraph_names.push(
                source
                    .and_then(|(i, _)| self.paragraph_style_names.get(i).cloned())
                    .flatten(),
            );
            paragraphs.push(ParagraphRun {
                range: start..start + piece.len(),
                style: None,
                local,
            });
            start += piece.len();
        }
        let notes = text
            .chars()
            .filter(|c| Marker::of(*c) == Some(Marker::FootnoteReference))
            .count();
        let mut footnotes = self.footnotes;
        footnotes.resize_with(notes, Story::new_footnote);
        let story = Story {
            text,
            runs,
            paragraphs,
            footnotes,
            index_entries: Vec::<IndexEntry>::new(),
            anchors: Vec::new(),
            cross_references: Vec::new(),
        };
        if story.runs_are_sound() && story.notes_are_sound() {
            (story, run_names, paragraph_names)
        } else {
            let mut plain = Story::new(story.text.clone());
            plain.footnotes = story.footnotes;
            let runs = vec![None; plain.runs.len()];
            let paragraphs = vec![None; plain.paragraphs.len()];
            (plain, runs, paragraphs)
        }
    }
}

type StyleTable = HashMap<String, (String, bool, ParagraphFormat, CharacterFormat)>;

fn read_body(
    body: Node,
    styles: &StyleTable,
    footnotes: &HashMap<String, Story>,
    b: &mut Builder,
    dropped: &mut Dropped,
) {
    let paragraphs: Vec<Node> = body
        .descendants()
        .filter(|n| n.tag_name().name() == "p")
        // Not the paragraphs inside a table's cells twice: a cell's are
        // reached through the table below.
        .filter(|n| !n.ancestors().skip(1).any(|a| a.tag_name().name() == "tbl"))
        .collect();
    let tables = body
        .descendants()
        .filter(|n| n.tag_name().name() == "tbl")
        .count();
    if tables > 0 {
        dropped.note("a table (tables are not imported yet); its text was kept");
    }
    // Walk the body in order, so a table's text lands where the table was.
    let mut first = true;
    for node in body.children().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "p" => {
                if !first {
                    b.text.push('\n');
                }
                first = false;
                read_paragraph(node, styles, footnotes, b);
            }
            "tbl" => {
                for row in node.children().filter(|n| n.tag_name().name() == "tr") {
                    if !first {
                        b.text.push('\n');
                    }
                    first = false;
                    let start = b.text.len();
                    for cell in row.children().filter(|n| n.tag_name().name() == "tc") {
                        for p in cell.children().filter(|n| n.tag_name().name() == "p") {
                            read_paragraph(p, styles, footnotes, b);
                            b.text.push('\t');
                        }
                    }
                    b.paragraphs
                        .push((start, b.text.len() + 1, ParagraphFormat::default()));
                    b.paragraph_style_names.push(None);
                }
            }
            "sdt" => {
                // A content control: its paragraphs are inside `sdtContent`.
                if let Some(content) = child(node, "sdtContent") {
                    read_body(content, styles, footnotes, b, dropped);
                }
            }
            _ => {}
        }
    }
    let _ = paragraphs;
}

fn read_paragraph(
    p: Node,
    styles: &StyleTable,
    footnotes: &HashMap<String, Story>,
    b: &mut Builder,
) {
    let start = b.text.len();
    let (style_name, mut format) = match child(p, "pPr") {
        Some(ppr) => {
            let name = child(ppr, "pStyle")
                .and_then(|s| attr(s, "val"))
                .and_then(|id| styles.get(id))
                .filter(|(_, is_paragraph, _, _)| *is_paragraph)
                .map(|(name, _, _, _)| name.clone());
            (name, paragraph_props(ppr))
        }
        None => (None, ParagraphFormat::default()),
    };
    // Direct run formatting on the paragraph mark applies to nothing here.
    format.character = CharacterFormat::default();

    for node in p.children().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "r" => read_run(node, styles, footnotes, b),
            "hyperlink" | "smartTag" | "ins" => {
                for run in node.children().filter(|n| n.tag_name().name() == "r") {
                    read_run(run, styles, footnotes, b);
                }
            }
            _ => {}
        }
    }
    b.paragraphs.push((start, b.text.len() + 1, format));
    b.paragraph_style_names.push(style_name);
}

fn read_run(r: Node, styles: &StyleTable, footnotes: &HashMap<String, Story>, b: &mut Builder) {
    let start = b.text.len();
    let (style_name, format) = match child(r, "rPr") {
        Some(rpr) => {
            let name = child(rpr, "rStyle")
                .and_then(|s| attr(s, "val"))
                .and_then(|id| styles.get(id))
                .filter(|(_, is_paragraph, _, _)| !*is_paragraph)
                .map(|(name, _, _, _)| name.clone());
            (name, run_props(rpr))
        }
        None => (None, CharacterFormat::default()),
    };
    for node in r.children().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "t" => b.text.push_str(node.text().unwrap_or("")),
            "tab" => b.text.push('\t'),
            "br" => b.text.push('\n'),
            "noBreakHyphen" => b.text.push('\u{2011}'),
            "softHyphen" => b.text.push('\u{00AD}'),
            "sym" => {
                if let Some(c) = attr(node, "char")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .and_then(char::from_u32)
                {
                    b.text.push(c);
                }
            }
            "footnoteReference" => {
                if let Some(note) = attr(node, "id").and_then(|id| footnotes.get(id)) {
                    b.text.push(Marker::FootnoteReference.character());
                    b.footnotes.push(note.clone());
                }
            }
            _ => {}
        }
    }
    if b.text.len() > start {
        b.runs.push((start, b.text.len(), format));
        b.run_style_names.push(style_name);
    }
}

/// `<w:pPr>` into paragraph formatting.
#[allow(clippy::field_reassign_with_default)]
fn paragraph_props(ppr: Node) -> ParagraphFormat {
    let mut f = ParagraphFormat::default();
    f.alignment = child(ppr, "jc").and_then(|jc| match attr(jc, "val") {
        Some("center") => Some(Alignment::Centre),
        Some("right") | Some("end") => Some(Alignment::Right),
        Some("both") | Some("distribute") => Some(Alignment::Justify),
        Some("left") | Some("start") => Some(Alignment::Left),
        _ => None,
    });
    if let Some(spacing) = child(ppr, "spacing") {
        f.space_before = attr(spacing, "before").and_then(twips);
        f.space_after = attr(spacing, "after").and_then(twips);
        if let (Some(line), rule) = (
            attr(spacing, "line").and_then(|v| v.parse::<f32>().ok()),
            attr(spacing, "lineRule"),
        ) && rule.is_none_or(|r| r == "auto")
        {
            // 240ths of a line.
            f.character.line_height = Some(line / 240.0);
        }
    }
    if let Some(ind) = child(ppr, "ind") {
        f.indent_left = attr(ind, "left").or(attr(ind, "start")).and_then(twips);
        f.indent_right = attr(ind, "right").or(attr(ind, "end")).and_then(twips);
        f.indent_first = attr(ind, "firstLine")
            .and_then(twips)
            .or_else(|| attr(ind, "hanging").and_then(twips).map(|h| -h));
    }
    if let Some(keep) = child(ppr, "keepNext")
        && attr(keep, "val") != Some("0")
        && attr(keep, "val") != Some("false")
    {
        f.keep = Some(tessera_text::story::KeepOptions {
            with_next: true,
            together: tessera_text::story::KeepTogether::Off,
        });
    }
    f
}

/// `<w:rPr>` into character formatting.
fn run_props(rpr: Node) -> CharacterFormat {
    let mut f = CharacterFormat::default();
    let on = |name: &str| -> bool {
        child(rpr, name).is_some_and(|n| !matches!(attr(n, "val"), Some("0") | Some("false")))
    };
    if on("b") {
        f.weight = Some(700);
    }
    if on("i") {
        f.italic = Some(true);
    }
    if on("caps") {
        f.case = Some(tessera_text::story::Case::Upper);
    }
    if on("smallCaps") {
        f.case = Some(tessera_text::story::Case::SmallCaps);
    }
    if on("strike") {
        f.strikethrough = Some(Decoration::default());
    }
    if let Some(u) = child(rpr, "u")
        && attr(u, "val") != Some("none")
    {
        f.underline = Some(Decoration::default());
    }
    if let Some(sz) = child(rpr, "sz")
        .and_then(|s| attr(s, "val"))
        .and_then(|v| v.parse::<f32>().ok())
    {
        f.size = Some(sz / 2.0);
    }
    if let Some(fonts) = child(rpr, "rFonts") {
        f.family = attr(fonts, "ascii")
            .or(attr(fonts, "hAnsi"))
            .map(str::to_owned);
    }
    if let Some(colour) = child(rpr, "color").and_then(|c| attr(c, "val"))
        && colour != "auto"
        && colour.len() == 6
        && let Ok(rgb) = u32::from_str_radix(colour, 16)
    {
        f.colour = Some(tessera_color::Color::Rgb {
            r: ((rgb >> 16) & 0xff) as f32 / 255.0,
            g: ((rgb >> 8) & 0xff) as f32 / 255.0,
            b: (rgb & 0xff) as f32 / 255.0,
            a: 1.0,
        });
    }
    if let Some(spacing) = child(rpr, "spacing")
        .and_then(|s| attr(s, "val"))
        .and_then(|v| v.parse::<f32>().ok())
    {
        // Twentieths of a point, to thousandths of an em at the run's size.
        let size = f.size.unwrap_or(12.0);
        f.tracking = Some(spacing / 20.0 / size * 1000.0);
    }
    if let Some(lang) = child(rpr, "lang").and_then(|l| attr(l, "val")) {
        f.language = lang.split('-').next().map(|l| l.to_ascii_lowercase());
    }
    f
}

fn twips(value: &str) -> Option<f32> {
    value.parse::<f32>().ok().map(|v| v / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, text) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("entry");
            zip.write_all(text.as_bytes()).expect("write");
        }
        zip.finish().expect("finish").into_inner()
    }

    const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn paragraphs_runs_and_styles_come_through() {
        let bytes = package(&[
            (
                "word/styles.xml",
                &format!(
                    r#"<w:styles {NS}><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:pPr><w:spacing w:before="240"/></w:pPr><w:rPr><w:sz w:val="32"/><w:b/></w:rPr></w:style></w:styles>"#
                ),
            ),
            (
                "word/document.xml",
                &format!(
                    r#"<w:document {NS}><w:body>
<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Alpha</w:t></w:r></w:p>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t xml:space="preserve">Plain and </w:t></w:r><w:r><w:rPr><w:i/><w:sz w:val="20"/></w:rPr><w:t>slanted</w:t></w:r></w:p>
</w:body></w:document>"#
                ),
            ),
        ]);
        let imported = import_bytes(bytes, Path::new("x.docx")).expect("import");
        assert_eq!(imported.story.text, "Alpha\nPlain and slanted");
        assert!(imported.story.runs_are_sound());
        assert_eq!(
            imported.paragraph_style_names,
            vec![Some("Heading 1".to_owned()), None]
        );
        assert_eq!(imported.paragraph_styles.len(), 1);
        assert_eq!(imported.paragraph_styles[0].format.space_before, Some(12.0));
        assert_eq!(
            imported.paragraph_styles[0].format.character.size,
            Some(16.0)
        );
        assert_eq!(
            imported.paragraph_styles[0].format.character.weight,
            Some(700)
        );
        assert_eq!(
            imported.story.paragraphs[1].local.alignment,
            Some(Alignment::Centre)
        );
        let slanted = imported
            .story
            .runs
            .iter()
            .find(|r| r.local.italic == Some(true))
            .expect("italic run");
        assert_eq!(slanted.local.size, Some(10.0));
        assert!(imported.dropped.is_empty());
    }

    #[test]
    fn footnotes_come_with_their_references() {
        let bytes = package(&[
            (
                "word/footnotes.xml",
                &format!(
                    r#"<w:footnotes {NS}><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>The source.</w:t></w:r></w:p></w:footnote></w:footnotes>"#
                ),
            ),
            (
                "word/document.xml",
                &format!(
                    r#"<w:document {NS}><w:body><w:p><w:r><w:t>A claim</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:t>.</w:t></w:r></w:p></w:body></w:document>"#
                ),
            ),
        ]);
        let imported = import_bytes(bytes, Path::new("x.docx")).expect("import");
        assert_eq!(
            imported.story.text,
            format!("A claim{}.", Marker::FootnoteReference.character())
        );
        assert!(imported.story.notes_are_sound());
        assert!(imported.story.footnotes[0].text.ends_with("The source."));
    }

    #[test]
    fn a_table_is_kept_as_tabbed_text_and_said() {
        let bytes = package(&[(
            "word/document.xml",
            &format!(
                r#"<w:document {NS}><w:body><w:p><w:r><w:t>Before</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>After</w:t></w:r></w:p></w:body></w:document>"#
            ),
        )]);
        let imported = import_bytes(bytes, Path::new("x.docx")).expect("import");
        assert_eq!(imported.story.text, "Before\na\tb\t\nAfter");
        assert!(imported.story.runs_are_sound());
        assert_eq!(imported.dropped.0.len(), 1);
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_refused_by_name() {
        let err = import_bytes(b"not a zip".to_vec(), Path::new("x.docx")).unwrap_err();
        assert!(matches!(err, ImportError::NotAPackage(_, "Word", _)));
    }
}
