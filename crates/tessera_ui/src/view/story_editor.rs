//! The story editor: the words without the page.
//!
//! A plain text box over the whole story — every frame of the thread, the
//! overset included — for the pass where a person is fixing copy and does
//! not want the layout in the way. Markers show as the characters they are.
//!
//! ## The edit is applied as an edit, not as a replacement
//!
//! Writing the box's text back over the story would flatten every run to
//! the first one's formatting. Instead the stretch that changed is found —
//! common prefix, common suffix — and only that stretch is deleted and
//! retyped, through the story's own operations, so bold stays bold on
//! either side of the change and an anchored object keeps its marker. One
//! edit per OK, and one undo entry.
//!
//! ## Styles in the margin
//!
//! Each paragraph's style is named beside it, as InDesign's story editor
//! has it, with a `+` where the paragraph departs from its style. The names
//! are the story's as it was opened, carried through the draft by the same
//! diff that applies it: a paragraph before or after the change keeps its
//! own, and one typed inside it takes the style of the paragraph it was
//! typed in — which is the style it will have once applied.

use tessera_document::ids::StoryId;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct StoryEditorWindow {
    pub open: bool,
    pub story: Option<StoryId>,
    pub text: String,
    document: Option<crate::app::DocumentKey>,
    original: String,
    /// The story's paragraphs as it was opened, and what each is set in.
    styles: Vec<(std::ops::Range<usize>, String)>,
}

impl StoryEditorWindow {
    /// Open on the story being edited, or the selected text frame's.
    pub fn open(&mut self, state: &TesseraApp) {
        use tessera_document::nodes::FrameKind;
        let editing = state.active().editing.as_ref().and_then(|(id, _)| {
            crate::view::viewport::editing_story(state, *id, state.active().editing_cell)
        });
        let story = editing.or_else(|| {
            let id = state.active().selection.single()?;
            match state.active().document().frame(id).map(|f| &f.kind) {
                Some(FrameKind::Text { story, .. }) => Some(*story),
                // A path's text has no caret on the page; this box is how
                // its words are edited.
                Some(FrameKind::Path(_)) => {
                    state.active().document().path_text(id).map(|t| t.story)
                }
                _ => None,
            }
        });
        let Some(story) = story else { return };
        let Some(text) = state
            .active()
            .document()
            .story(story)
            .map(|s| s.text.clone())
        else {
            return;
        };
        self.styles = paragraph_styles(state.active().document(), story);
        self.story = Some(story);
        self.document = Some(state.active);
        self.original = text.clone();
        self.text = text;
        self.open = true;
    }

    fn conflict(&self, state: &TesseraApp) -> Option<&'static str> {
        if self.document != Some(state.active) {
            return Some("Switch back to the original document to apply this draft.");
        }
        let current = self
            .story
            .and_then(|story| state.active().document().story(story));
        match current {
            None => Some("This story was removed. Copy your draft before closing."),
            Some(story) if story.text != self.original => Some(
                "The story changed while this editor was open. Copy your draft and reopen the story to avoid overwriting newer text.",
            ),
            _ => None,
        }
    }

    fn apply_draft(&mut self, state: &mut TesseraApp) {
        if self.conflict(state).is_some() {
            return;
        }
        if let Some(story) = self.story
            && self.original != self.text
        {
            let (range, with) = minimal_edit(&self.original, &self.text);
            apply(
                state,
                Command::ReplaceMatches {
                    edits: vec![(story, range, with)],
                },
            );
        }
        self.open = false;
    }
}

/// Every paragraph of a story, with the name of its style: `[Basic
/// Paragraph]` for none, and a `+` after it where the paragraph's own
/// formatting overrides the style.
fn paragraph_styles(
    doc: &tessera_document::document::Document,
    story: StoryId,
) -> Vec<(std::ops::Range<usize>, String)> {
    let Some(story) = doc.story(story) else {
        return Vec::new();
    };
    story
        .paragraph_ranges()
        .into_iter()
        .map(|range| {
            let run = story.paragraph_run_at(range.start);
            let mut name = run
                .and_then(|r| r.style)
                .and_then(|s| doc.paragraph_styles.get(s))
                .map_or_else(|| "[Basic Paragraph]".to_owned(), |s| s.name.clone());
            if run.is_some_and(|r| r.local != Default::default()) {
                name.push('+');
            }
            (range, name)
        })
        .collect()
}

/// The style name for each paragraph of `draft`, read off the paragraphs of
/// `original` it came from: see the module's note on styles.
pub(crate) fn draft_styles(
    original: &str,
    draft: &str,
    styles: &[(std::ops::Range<usize>, String)],
) -> Vec<String> {
    let (changed, with) = minimal_edit(original, draft);
    let typed_end = changed.start + with.len();
    let starts = std::iter::once(0).chain(draft.match_indices('\n').map(|(at, _)| at + 1));
    starts
        .map(|at| {
            // Where this paragraph's start was before the edit. After what
            // was typed first: a start standing where text was only taken
            // out is the start of what followed it.
            let was = if at >= typed_end {
                at - with.len() + changed.len()
            } else if at <= changed.start {
                at
            } else {
                changed.start
            };
            styles
                .iter()
                .find(|(range, _)| range.start <= was && was < range.end)
                .or_else(|| styles.last())
                .map_or_else(String::new, |(_, name)| name.clone())
        })
        .collect()
}

/// The stretch `before` and `after` disagree on: what to take out of
/// `before`, and what to put in its place, at char boundaries.
pub(crate) fn minimal_edit(before: &str, after: &str) -> (std::ops::Range<usize>, String) {
    let prefix = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let prefix = (0..=prefix)
        .rev()
        .find(|p| before.is_char_boundary(*p) && after.is_char_boundary(*p))
        .unwrap_or(0);
    let max_suffix = (before.len() - prefix).min(after.len() - prefix);
    let suffix = before
        .bytes()
        .rev()
        .zip(after.bytes().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = (0..=suffix)
        .rev()
        .find(|s| {
            before.is_char_boundary(before.len() - s) && after.is_char_boundary(after.len() - s)
        })
        .unwrap_or(0);
    (
        prefix..before.len() - suffix,
        after[prefix..after.len() - suffix].to_owned(),
    )
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.story_editor.open {
        return;
    }
    let mut window = state.story_editor.clone();
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("story-editor"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            let size = ctx.content_rect();
            ui.set_width((size.width() - 64.0).clamp(360.0, 720.0));
            ui.heading("Story editor");
            ui.add_space(Theme::space_2());
            let names = draft_styles(&window.original, &window.text, &window.styles);
            egui::ScrollArea::vertical()
                .max_height((size.height() - 180.0).max(120.0))
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        // The margin the names stand in, left of the text.
                        let gutter = (ui.available_width() * 0.22).clamp(96.0, 160.0);
                        let left = ui.cursor().min.x;
                        ui.add_space(gutter);
                        let output = egui::TextEdit::multiline(&mut window.text)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(16)
                            .desired_width(f32::INFINITY)
                            .show(ui);
                        // A paragraph starts on the first row and on every
                        // row after one that ended with a newline.
                        let mut starts_paragraph = true;
                        let mut names = names.iter();
                        let font = egui::TextStyle::Small.resolve(ui.style());
                        for row in &output.galley.rows {
                            if starts_paragraph && let Some(name) = names.next() {
                                let y = output.galley_pos.y + row.rect().min.y;
                                let at = egui::pos2(left, y);
                                let room = egui::Rect::from_min_size(
                                    at,
                                    egui::vec2(gutter - Theme::space_2(), row.rect().height()),
                                );
                                ui.painter().with_clip_rect(room).text(
                                    at,
                                    egui::Align2::LEFT_TOP,
                                    name,
                                    font.clone(),
                                    Theme::text_muted(),
                                );
                            }
                            starts_paragraph = row.ends_with_newline;
                        }
                    });
                });
            ui.add_space(Theme::space_2());
            ui.weak(format!(
                "{} words · {} characters",
                window.text.split_whitespace().count(),
                window.text.chars().count()
            ));
            if let Some(conflict) = window.conflict(state) {
                ui.colored_label(Theme::error(), conflict);
            }
            ui.horizontal(|ui| {
                go = ui
                    .add_enabled(
                        window.conflict(state).is_none(),
                        super::primary_button("Apply changes"),
                    )
                    .clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });
    if response.should_close() {
        window.open = false;
    }
    if go {
        window.apply_draft(state);
    }
    state.story_editor = window;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_paragraph_of_a_draft_is_named_by_the_style_it_came_from() {
        let styles = vec![
            (0..8, "Heading".to_owned()),
            (8..20, "Body+".to_owned()),
            (20..30, "Caption".to_owned()),
        ];
        let original = "Heading\nBody text 1\nA caption";
        assert_eq!(
            draft_styles(original, original, &styles),
            ["Heading", "Body+", "Caption"]
        );
        // A paragraph broken in two: both halves are body text, and the
        // caption after them is still a caption.
        let split = "Heading\nBody\n text 1\nA caption";
        assert_eq!(
            draft_styles(original, split, &styles),
            ["Heading", "Body+", "Body+", "Caption"]
        );
        // The body taken out altogether: the heading and the caption meet.
        let cut = "Heading\nA caption";
        assert_eq!(draft_styles(original, cut, &styles), ["Heading", "Caption"]);
    }

    #[test]
    fn the_editor_names_a_paragraph_style_and_marks_an_override() {
        let mut state = TesseraApp::headless();
        let bounds = state.first_page_bounds();
        apply(&mut state, Command::AddTextFrame(bounds));
        let frame = state.active().selection.single().unwrap();
        apply(
            &mut state,
            Command::SetText {
                id: frame,
                text: "One\nTwo".into(),
            },
        );
        let story = match &state.active().document().frame(frame).unwrap().kind {
            tessera_document::nodes::FrameKind::Text { story, .. } => *story,
            _ => panic!("a text frame"),
        };
        let doc = state.active_mut().document_mut();
        let body = doc.add_paragraph_style(tessera_text::story::ParagraphStyle {
            name: "Body".into(),
            ..Default::default()
        });
        let s = doc.story_mut(story).unwrap();
        s.set_paragraph_style(4..7, Some(body));
        s.apply_paragraph_format(
            4..7,
            &tessera_text::story::ParagraphFormat {
                alignment: Some(tessera_text::story::Alignment::Centre),
                ..Default::default()
            },
        );
        let names: Vec<String> = paragraph_styles(state.active().document(), story)
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(names, ["[Basic Paragraph]", "Body+"]);
    }

    #[test]
    fn a_draft_cannot_overwrite_another_document_or_newer_text() {
        let mut state = TesseraApp::headless();
        let bounds = state.first_page_bounds();
        apply(&mut state, Command::AddTextFrame(bounds));
        let frame = state.active().selection.single().unwrap();
        apply(
            &mut state,
            Command::SetText {
                id: frame,
                text: "original".into(),
            },
        );
        let mut editor = StoryEditorWindow::default();
        editor.open(&state);
        editor.text = "my draft".into();
        let source = state.active;
        state.add_document(state.active().document().clone(), None);
        editor.apply_draft(&mut state);
        assert!(editor.open);
        let story = editor.story.unwrap();
        assert_eq!(
            state.active().document().story(story).unwrap().text,
            "original"
        );
        state.active = source;
        apply(
            &mut state,
            Command::SetText {
                id: frame,
                text: "newer copy".into(),
            },
        );
        editor.apply_draft(&mut state);
        assert!(editor.open);
        assert_eq!(editor.text, "my draft");
        assert_eq!(
            state.active().document().story(story).unwrap().text,
            "newer copy"
        );
    }

    #[test]
    fn the_edit_is_the_smallest_stretch_that_changed() {
        assert_eq!(
            minimal_edit("the cat sat", "the dog sat"),
            (4..7, "dog".into())
        );
        assert_eq!(minimal_edit("abc", "abXc"), (2..2, "X".into()));
        assert_eq!(minimal_edit("abXc", "abc"), (2..3, String::new()));
        assert_eq!(minimal_edit("same", "same"), (4..4, String::new()));
        // Never inside a character.
        let (range, with) = minimal_edit("caf\u{e9}s", "caf\u{e8}s");
        assert!("caf\u{e9}s".is_char_boundary(range.start));
        assert!("caf\u{e9}s".is_char_boundary(range.end));
        assert_eq!(with, "\u{e8}");
    }

    #[test]
    fn applying_keeps_the_formatting_either_side() {
        use tessera_document::nodes::FrameKind;
        use tessera_geometry::DocRect;
        use tessera_text::story::CharacterFormat;
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 20.0,
                y: 20.0,
                width: 300.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().unwrap();
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "bold cat plain".into(),
            },
        );
        let FrameKind::Text { story, .. } = state.active().document().frame(id).unwrap().kind
        else {
            panic!()
        };
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..4,
                format: CharacterFormat {
                    weight: Some(700),
                    ..Default::default()
                },
            },
        );
        let mut window = StoryEditorWindow::default();
        window.open(&state);
        assert_eq!(window.text, "bold cat plain");
        let (range, with) = minimal_edit(&window.text, "bold dog plain");
        apply(
            &mut state,
            Command::ReplaceMatches {
                edits: vec![(story, range, with)],
            },
        );
        let story = state.active().document().story(story).unwrap();
        assert_eq!(story.text, "bold dog plain");
        assert_eq!(
            story.run_at(0).unwrap().local.weight,
            Some(700),
            "bold stayed"
        );
        assert_eq!(
            story.run_at(6).unwrap().local.weight,
            None,
            "the new word is plain"
        );
    }
}
