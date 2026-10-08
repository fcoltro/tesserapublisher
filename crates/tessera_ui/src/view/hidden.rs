//! Type > Show Hidden Characters: the characters that set no ink, drawn
//! where they are in a colour that never prints.
//!
//! InDesign's marks, as typesetters read them: a raised dot for a word
//! space, a degree sign for a space that does not break, a double arrow for
//! a tab, a pilcrow where a paragraph ends, a hash where the story does, and
//! a lozenge for a marker — a page number, a variable, an anchor, a note —
//! whose text the layout writes in. Interface, not document, like the caret:
//! drawn by egui over the page, so it can never reach a PDF or a print.

use tessera_document::nodes::FrameKind;
use tessera_geometry::{DocPoint, DocRect, Transform};
use tessera_layout::resolve::ResolvedKind;
use tessera_text::edit::TextCursor;

use crate::app::TesseraApp;

/// One mark: what to draw, where its baseline begins in the frame's own
/// space, and how tall the line it sits on is.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub glyph: char,
    pub at: DocPoint,
    pub size: f64,
}

/// The marks of one frame, with the box and placement that put them on the
/// page.
pub struct Marks {
    pub bounds: DocRect,
    pub transform: Transform,
    pub marks: Vec<Mark>,
}

/// What a character is shown as, when it sets no ink of its own.
pub fn glyph_for(c: char) -> Option<char> {
    match c {
        ' ' => Some('\u{00B7}'),
        '\u{00A0}' | '\u{202F}' => Some('\u{00B0}'),
        '\u{2000}'..='\u{200A}' => Some('\u{00B7}'),
        '\t' => Some('\u{00BB}'),
        '\n' => Some('\u{00B6}'),
        '\u{00AD}' => Some('-'),
        '\u{E000}'..='\u{F8FF}' => Some('\u{25CA}'),
        _ => None,
    }
}

/// Every hidden character the canvas shows, frame by frame.
pub fn marks(state: &mut TesseraApp) -> Vec<Marks> {
    let key = state.active;
    let items: Vec<_> = state
        .resolve_active()
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ResolvedKind::Text { shaped, .. } => {
                Some((item.frame, item.bounds, item.transform, shaped.clone()))
            }
            _ => None,
        })
        .collect();
    let doc = state.documents[key].document();
    let mut out = Vec::new();
    for (frame, bounds, transform, shaped) in items {
        let Some(FrameKind::Text { story, .. }) = doc.frame(frame).map(|f| &f.kind) else {
            continue;
        };
        let Some(text) = doc.story(*story).map(|s| s.text.as_str()) else {
            continue;
        };
        let mut marks = Vec::new();
        for line in &shaped.lines {
            let size = (line.ascent + line.descent).max(1.0);
            let shown = &text[line.range.start.min(text.len())..line.range.end.min(text.len())];
            for (offset, c) in shown.char_indices() {
                let Some(glyph) = glyph_for(c) else {
                    continue;
                };
                let at = line.range.start + offset;
                let geometry = shaped.caret_geometry(
                    TextCursor {
                        position: at + c.len_utf8(),
                        anchor: at,
                    },
                    1.0,
                );
                // A space's own width, its mark in the middle; a paragraph's
                // end, a tab and a marker from where they begin.
                let place = match geometry.selection.first() {
                    Some(r) if c == ' ' || glyph == '\u{00B7}' || glyph == '\u{00B0}' => {
                        (r.x0 + r.x1) / 2.0 - size * 0.12
                    }
                    Some(r) => r.x0,
                    None => match geometry.caret {
                        Some(r) => r.x0,
                        None => continue,
                    },
                };
                marks.push(Mark {
                    glyph,
                    at: DocPoint {
                        x: place,
                        y: line.baseline,
                    },
                    size,
                });
            }
            // A paragraph's end, which the line's own range stops short of.
            if text[line.range.end.min(text.len())..].starts_with('\n')
                && !shown.ends_with('\n')
                && let Some(r) = shaped
                    .caret_geometry(
                        TextCursor {
                            position: line.range.end,
                            anchor: line.range.end,
                        },
                        1.0,
                    )
                    .caret
            {
                marks.push(Mark {
                    glyph: '\u{00B6}',
                    at: DocPoint {
                        x: r.x0 + size * 0.05,
                        y: line.baseline,
                    },
                    size,
                });
            }
            // The end of the story, after its last character.
            if line.range.end >= text.len()
                && !text.ends_with('\n')
                && let Some(r) = shaped
                    .caret_geometry(
                        TextCursor {
                            position: text.len(),
                            anchor: text.len(),
                        },
                        1.0,
                    )
                    .caret
            {
                marks.push(Mark {
                    glyph: '#',
                    at: DocPoint {
                        x: r.x0 + size * 0.05,
                        y: line.baseline,
                    },
                    size,
                });
            }
        }
        if !marks.is_empty() {
            out.push(Marks {
                bounds,
                transform,
                marks,
            });
        }
    }
    out
}

/// Draw the marks, in the colour of the frame edges: furniture, like them.
pub fn draw(
    painter: &egui::Painter,
    to_screen: &dyn Fn(DocPoint) -> egui::Pos2,
    zoom: f64,
    all: &[Marks],
    colour: egui::Color32,
) {
    for frame in all {
        for mark in &frame.marks {
            let at = to_screen(frame.transform.apply(DocPoint {
                x: frame.bounds.x + mark.at.x,
                y: frame.bounds.y + mark.at.y,
            }));
            let px = (mark.size * zoom * 0.7).clamp(4.0, 200.0) as f32;
            painter.text(
                at,
                egui::Align2::LEFT_BOTTOM,
                mark.glyph,
                egui::FontId::proportional(px),
                colour,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply};

    #[test]
    fn each_kind_of_invisible_has_its_mark_and_ink_has_none() {
        assert_eq!(glyph_for(' '), Some('\u{00B7}'));
        assert_eq!(glyph_for('\t'), Some('\u{00BB}'));
        assert_eq!(glyph_for('\n'), Some('\u{00B6}'));
        assert_eq!(glyph_for('\u{00A0}'), Some('\u{00B0}'));
        assert_eq!(glyph_for('\u{E000}'), Some('\u{25CA}'));
        assert_eq!(glyph_for('a'), None);
        assert_eq!(glyph_for('.'), None);
    }

    #[test]
    fn a_story_shows_its_spaces_its_paragraphs_and_its_end() {
        let mut state = TesseraApp::headless();
        let page = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: page.x + 36.0,
                y: page.y + 36.0,
                width: 300.0,
                height: 200.0,
            }),
        );
        let id = state.active().selection.single().expect("a frame");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "one two\tthree\nfour".to_string(),
            },
        );
        let all = marks(&mut state);
        let glyphs: Vec<char> = all
            .iter()
            .flat_map(|m| m.marks.iter().map(|k| k.glyph))
            .collect();
        assert_eq!(
            glyphs,
            vec!['\u{00B7}', '\u{00BB}', '\u{00B6}', '#'],
            "a space, a tab, a paragraph's end and the story's"
        );
        // Left to right along the first line.
        let first = &all[0].marks;
        assert!(first[0].at.x < first[1].at.x && first[1].at.x < first[2].at.x);
        // The second paragraph is below the first.
        assert!(first[3].at.y > first[0].at.y);
    }
}
