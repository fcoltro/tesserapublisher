//! `Stories/Story_*.xml` into a [`Story`].
//!
//! A story in IDML is paragraph-style ranges holding character-style ranges
//! holding content, with `<Br/>` for a paragraph break. Tessera's story is
//! flat text with two run lists over it, so the ranges are walked once,
//! pushing text and remembering where each range started and stopped, and
//! the lists are made from those marks at the end — which is what lets a
//! footnote in the middle of a range, or a marker, be one character in the
//! text like any other.
//!
//! What is not carried is said: a table, an anchored object, a nested
//! group — each becomes a line in [`crate::Dropped`] rather than a gap.

use roxmltree::Node;
use tessera_text::story::{CharacterFormat, IndexEntry, ParagraphFormat, ParagraphRun, Run, Story};
use tessera_text::variables::Marker;

use super::styles::{Colours, Styles, character_format, paragraph_format};
use crate::xml::attr;

/// A story, and the objects set into its text.
///
/// Each inline object — a picture, a shape, a table — is a `U+FFFC` marker
/// in the text and a node here, by the marker's index, for the caller to
/// make a frame of and anchor. The node is kept rather than the frame
/// because a frame needs the document, and the story does not.
pub(crate) struct Read<'a, 'i> {
    pub story: Story,
    pub inline: Vec<(usize, Node<'a, 'i>)>,
}

/// The story a `<Story>` element describes.
pub(crate) fn read<'a, 'i>(
    story: Node<'a, 'i>,
    styles: &Styles,
    colours: &Colours,
) -> Read<'a, 'i> {
    let mut b = Builder::default();
    read_ranges(story, styles, colours, &mut b);
    let inline = std::mem::take(&mut b.inline);
    Read {
        story: b.finish(),
        inline,
    }
}

#[derive(Default)]
struct Builder<'a, 'i> {
    text: String,
    /// The objects set into the text, by marker index.
    inline: Vec<(usize, Node<'a, 'i>)>,
    /// `(start, end, style, local)` for every character range met.
    runs: Vec<(
        usize,
        usize,
        Option<tessera_text::story::CharacterStyleId>,
        CharacterFormat,
    )>,
    /// The same for paragraph ranges; a paragraph takes the range its first
    /// byte falls in.
    paragraphs: Vec<(
        usize,
        usize,
        Option<tessera_text::story::ParagraphStyleId>,
        ParagraphFormat,
    )>,
    footnotes: Vec<Story>,
}

impl<'a, 'i> Builder<'a, 'i> {
    /// Put a marker for an inline object here, and remember the node.
    fn push_inline(&mut self, node: Node<'a, 'i>) {
        let index = self
            .text
            .chars()
            .filter(|c| *c == tessera_document::anchored::MARKER)
            .count();
        self.text.push(tessera_document::anchored::MARKER);
        self.inline.push((index, node));
    }

    fn finish(self) -> Story {
        let text = self.text;
        if text.is_empty() {
            return Story::default();
        }

        // Runs: the ranges met, clipped to the text, gaps filled with plain.
        let mut runs: Vec<Run> = Vec::new();
        let mut at = 0usize;
        for (start, end, style, local) in self.runs {
            let start = start.max(at).min(text.len());
            let end = end.min(text.len());
            if start > at {
                runs.push(Run::plain(at..start));
            }
            if end > start {
                runs.push(Run {
                    range: start..end,
                    style,
                    local,
                });
                at = end;
            }
        }
        if at < text.len() {
            runs.push(Run::plain(at..text.len()));
        }

        // Paragraphs: one per line of the text, in the paragraph range its
        // start falls in.
        let mut paragraphs: Vec<ParagraphRun> = Vec::new();
        let mut start = 0usize;
        for piece in text.split_inclusive('\n') {
            let range = start..start + piece.len();
            let found = self
                .paragraphs
                .iter()
                .find(|(s, e, _, _)| *s <= start && start < *e)
                .or_else(|| self.paragraphs.last());
            let (style, local) = found
                .map(|(_, _, style, local)| (*style, local.clone()))
                .unwrap_or((None, ParagraphFormat::default()));
            paragraphs.push(ParagraphRun {
                range,
                style,
                local,
            });
            start += piece.len();
        }
        if text.ends_with('\n') {
            // A story ending in a break has an empty last paragraph, which
            // the run invariant does not count: nothing to push.
        }

        let notes = text
            .chars()
            .filter(|c| Marker::of(*c) == Some(Marker::FootnoteReference))
            .count();
        let mut footnotes = self.footnotes;
        footnotes.resize_with(notes, Story::new_footnote);
        let entries = text
            .chars()
            .filter(|c| Marker::of(*c) == Some(Marker::IndexEntry))
            .count();

        let story = Story {
            text,
            runs,
            paragraphs,
            footnotes,
            index_entries: vec![IndexEntry::default(); entries],
        };
        if story.runs_are_sound() && story.notes_are_sound() {
            story
        } else {
            // The arithmetic went wrong somewhere; the words are worth more
            // than the formatting, so keep them.
            let mut plain = Story::new(story.text.clone());
            plain.footnotes = story.footnotes;
            plain
        }
    }
}

fn read_ranges<'a, 'i>(
    node: Node<'a, 'i>,
    styles: &Styles,
    colours: &Colours,
    b: &mut Builder<'a, 'i>,
) {
    for child in node.children() {
        if child.is_pi() {
            push_instruction(child, b);
            continue;
        }
        if !child.is_element() {
            continue;
        }
        match child.tag_name().name() {
            "ParagraphStyleRange" => {
                let start = b.text.len();
                let style = attr(child, "AppliedParagraphStyle")
                    .and_then(|s| styles.paragraph.get(s))
                    .copied();
                let local = paragraph_format(child, colours);
                read_ranges(child, styles, colours, b);
                b.paragraphs.push((start, b.text.len(), style, local));
            }
            "CharacterStyleRange" => {
                let start = b.text.len();
                let style = attr(child, "AppliedCharacterStyle")
                    .and_then(|s| styles.character.get(s))
                    .copied();
                let local = character_format(child, colours, None);
                read_ranges(child, styles, colours, b);
                b.runs.push((start, b.text.len(), style, local));
            }
            "Content" => {
                for piece in child.children() {
                    if piece.is_pi() {
                        push_instruction(piece, b);
                    } else if let Some(text) = piece.text() {
                        // InDesign writes a forced line break as U+2028 and
                        // a paragraph break only as <Br/>; the line break
                        // is a newline to Tessera too, which is near enough.
                        b.text.push_str(&text.replace('\u{2028}', "\n"));
                    }
                }
            }
            "Br" => b.text.push('\n'),
            "Footnote" => {
                b.text.push(Marker::FootnoteReference.character());
                let mut note: Builder<'a, 'i> = Builder::default();
                read_ranges(child, styles, colours, &mut note);
                let mut note = note.finish();
                // A note that did not carry its own number gets one.
                if !note.text.starts_with(Marker::FootnoteNumber.character()) {
                    note.insert_text(0, &format!("{}\t", Marker::FootnoteNumber.character()));
                }
                b.footnotes.push(note);
            }
            // An object set into the text — a picture, a shape, a table —
            // is a marker here and a frame anchored to it later.
            "Table" | "Rectangle" | "Oval" | "Polygon" | "TextFrame" | "Group" | "GraphicLine" => {
                b.push_inline(child);
            }
            "HyperlinkTextSource" | "HyperlinkTextDestination" | "XMLElement" | "Change" => {
                // Wrappers around ordinary ranges: read through them.
                read_ranges(child, styles, colours, b);
            }
            _ => {}
        }
    }
}

/// `<?ACE n?>`: InDesign's special characters.
///
/// 18 is the current page number, 19 the section marker, 4 the number at the
/// head of a footnote. 16 and 17 are the next and previous page numbers.
/// The rest — indent-to-here, right-indent tab, nested-style ends — have no
/// character in Tessera and are dropped without a note, because they change
/// layout, not words.
fn push_instruction(node: Node, b: &mut Builder) {
    let Some(pi) = node.pi().filter(|pi| pi.target == "ACE") else {
        return;
    };
    let marker = match pi.value.map(str::trim) {
        Some("18") => Marker::PageNumber,
        Some("19") => Marker::SectionMarker,
        Some("16") => Marker::NextPageNumber,
        Some("17") => Marker::PreviousPageNumber,
        Some("4") => Marker::FootnoteNumber,
        _ => return,
    };
    b.text.push(marker.character());
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::document::Document;

    fn story_from(xml: &str) -> (Story, usize) {
        let doc = roxmltree::Document::parse(xml).expect("xml");
        let colours = Colours::default();
        let mut tessera = Document::default();
        let styles = Styles::read(doc.root(), &mut tessera, &colours);
        let read = read(doc.root_element(), &styles, &colours);
        (read.story, read.inline.len())
    }

    #[test]
    fn ranges_become_text_runs_and_paragraphs() {
        let (story, _) = story_from(
            r#"<Story Self="u1">
  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/Heading" Justification="CenterAlign">
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" PointSize="24">
      <Content>Alpha</Content><Br/>
    </CharacterStyleRange>
  </ParagraphStyleRange>
  <ParagraphStyleRange AppliedParagraphStyle="ParagraphStyle/$ID/NormalParagraphStyle">
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]">
      <Content>Plain and </Content>
    </CharacterStyleRange>
    <CharacterStyleRange AppliedCharacterStyle="CharacterStyle/$ID/[No character style]" FontStyle="Italic">
      <Content>slanted</Content>
    </CharacterStyleRange>
  </ParagraphStyleRange>
</Story>"#,
        );
        assert_eq!(story.text, "Alpha\nPlain and slanted");
        assert!(story.runs_are_sound());
        assert_eq!(story.paragraphs.len(), 2);
        assert_eq!(
            story.paragraphs[0].local.alignment,
            Some(tessera_text::story::Alignment::Centre)
        );
        assert_eq!(story.runs.len(), 3);
        assert_eq!(story.runs[0].local.size, Some(24.0));
        assert_eq!(story.runs[2].local.italic, Some(true));
    }

    #[test]
    fn a_footnote_and_a_page_number_come_through_as_markers() {
        let (story, _) = story_from(
            r#"<Story Self="u1">
  <ParagraphStyleRange>
    <CharacterStyleRange>
      <Content>Page <?ACE 18?> cites</Content>
      <Footnote>
        <ParagraphStyleRange><CharacterStyleRange><Content><?ACE 4?>	The source.</Content></CharacterStyleRange></ParagraphStyleRange>
      </Footnote>
      <Content> here.</Content>
    </CharacterStyleRange>
  </ParagraphStyleRange>
</Story>"#,
        );
        let expected = format!(
            "Page {} cites{} here.",
            Marker::PageNumber.character(),
            Marker::FootnoteReference.character()
        );
        assert_eq!(story.text, expected);
        assert!(story.notes_are_sound());
        assert_eq!(story.footnotes.len(), 1);
        assert!(
            story.footnotes[0]
                .text
                .starts_with(Marker::FootnoteNumber.character())
        );
        assert!(story.footnotes[0].text.ends_with("The source."));
    }

    #[test]
    fn a_table_becomes_a_marker_with_its_node_kept_for_later() {
        let (story, inline) = story_from(
            r#"<Story Self="u1"><ParagraphStyleRange><CharacterStyleRange>
  <Content>Before</Content><Br/>
  <Table><Row/><Cell><ParagraphStyleRange><CharacterStyleRange><Content>a1</Content></CharacterStyleRange></ParagraphStyleRange></Cell>
  <Cell><ParagraphStyleRange><CharacterStyleRange><Content>b1</Content></CharacterStyleRange></ParagraphStyleRange></Cell></Table>
  <Content>After</Content>
</CharacterStyleRange></ParagraphStyleRange></Story>"#,
        );
        assert_eq!(
            story.text,
            format!("Before\n{}After", tessera_document::anchored::MARKER)
        );
        assert_eq!(inline, 1, "one object to anchor");
        assert!(story.runs_are_sound());
    }
}
