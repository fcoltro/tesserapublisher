//! The long-document boxes: a footnote's text, an index entry's topic, and
//! the recipes for the table of contents and the index.
//!
//! Four small modals rather than panels. Each is opened for one thing, does
//! it on OK, and goes away — a footnote is written once and an index is
//! regenerated once a chapter — so a docked panel would be a strip of empty
//! fields most of the day.
//!
//! ## Footnotes are edited in a box, not on the canvas
//!
//! The note's lines are drawn in the frame with no hit geometry, so a click
//! on them lands nowhere. That is deliberate for now: on-canvas editing of a
//! note means a second caret model inside the first, and the box gets the
//! words onto the page today. The number at the note's head is a marker and
//! is not shown here — deleting it is a choice made in the story, not a slip
//! of the box.

use egui::Ui;
use tessera_document::contents::{Contents, Index, Level};
use tessera_document::ids::StoryId;
use tessera_text::story::ParagraphStyleId;
use tessera_text::variables::Marker;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// The footnote box: which note, and its words as the fields hold them.
#[derive(Debug, Clone, Default)]
pub struct FootnoteWindow {
    pub open: bool,
    pub story: Option<StoryId>,
    pub index: usize,
    pub text: String,
}

/// The index-entry box: the topic to file the caret's place under.
#[derive(Debug, Clone, Default)]
pub struct IndexEntryWindow {
    pub open: bool,
    pub topic: String,
}

/// The contents box, with the recipe as the fields hold it.
#[derive(Debug, Clone, Default)]
pub struct ContentsWindow {
    pub open: bool,
    pub draft: Option<Contents>,
}

/// The index box.
#[derive(Debug, Clone, Default)]
pub struct IndexWindow {
    pub open: bool,
    pub draft: Option<Index>,
}

/// The endnotes box.
#[derive(Debug, Default, Clone)]
pub struct EndnotesWindow {
    pub open: bool,
    pub draft: Option<tessera_document::contents::Endnotes>,
}

/// The note's words without the number and tab a fresh note begins with.
///
/// What the box shows and what it writes back around; the prefix is kept
/// exactly as it was, so a note somebody has already reworded keeps its shape.
pub(crate) fn split_prefix(text: &str) -> (&str, &str) {
    let number = Marker::FootnoteNumber.character();
    let prefix_len = if text.starts_with(number) {
        let after = number.len_utf8();
        if text[after..].starts_with('\t') {
            after + 1
        } else {
            after
        }
    } else {
        0
    };
    text.split_at(prefix_len)
}

/// The footnote the caret is at or just past: the last reference marker at or
/// before the cursor, or the first after it when there is none before.
pub(crate) fn footnote_at_caret(state: &TesseraApp) -> Option<(StoryId, usize)> {
    let (id, buffer) = state.active().editing.as_ref()?;
    let story = crate::view::viewport::editing_story(state, *id, state.active().editing_cell)?;
    let offsets = buffer.story().footnote_offsets();
    if offsets.is_empty() {
        return None;
    }
    let at = buffer.cursor().position;
    let index = offsets.iter().rposition(|o| *o < at).unwrap_or(0);
    Some((story, index))
}

impl FootnoteWindow {
    pub fn open(&mut self, state: &TesseraApp) {
        let Some((story, index)) = footnote_at_caret(state) else {
            return;
        };
        let Some(note) = state
            .active()
            .document()
            .story(story)
            .and_then(|s| s.footnotes.get(index))
        else {
            return;
        };
        self.story = Some(story);
        self.index = index;
        self.text = split_prefix(&note.text).1.to_owned();
        self.open = true;
    }
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    footnote(ctx, state);
    index_entry(ctx, state);
    endnotes(ctx, state);
    contents(ctx, state);
    index(ctx, state);
}

fn footnote(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.footnote.open {
        return;
    }
    let mut window = state.footnote.clone();
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("footnote"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 480.0));
            ui.heading(format!("Footnote {}", window.index + 1));
            ui.add_space(Theme::space_2());
            ui.add(
                egui::TextEdit::multiline(&mut window.text)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                go = ui.add(super::primary_button("OK")).clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });
    if response.should_close() {
        window.open = false;
    }
    if go {
        if let Some(story) = window.story {
            apply(
                state,
                Command::SetFootnoteText {
                    story,
                    index: window.index,
                    text: window.text.clone(),
                },
            );
        }
        window.open = false;
    }
    state.footnote = window;
}

fn index_entry(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.index_entry.open {
        return;
    }
    let mut window = state.index_entry.clone();
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("index-entry"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 420.0));
            ui.heading("Index entry");
            ui.add_space(Theme::space_2());
            crate::view::panels::field(ui, "Topic", |ui| {
                ui.add(egui::TextEdit::singleline(&mut window.topic).desired_width(f32::INFINITY));
            });
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                go = ui
                    .add_enabled(
                        !window.topic.trim().is_empty(),
                        super::primary_button("Add"),
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
    state.index_entry = window;
    if go {
        insert_index_entry(state, &state.index_entry.topic.clone());
        state.index_entry.open = false;
    }
}

/// Put an index marker at the caret, filed under `topic`.
///
/// At the *start* of a selection, and without replacing it: the words a
/// person selected are what they want indexed, not what they want gone.
pub(crate) fn insert_index_entry(state: &mut TesseraApp, topic: &str) -> bool {
    let Some((id, buffer)) = state.active_mut().editing.as_mut() else {
        return false;
    };
    let id = *id;
    if let Some(range) = buffer.selection_range() {
        buffer.set_cursor(range.start);
    }
    let at = buffer.cursor().position;
    if !crate::view::viewport::type_text(state, &Marker::IndexEntry.character().to_string()) {
        return false;
    }
    let Some(story) = crate::view::viewport::editing_story(state, id, state.active().editing_cell)
    else {
        return false;
    };
    let index = state
        .active()
        .document()
        .story(story)
        .map(|s| s.index_entry_at(at))
        .unwrap_or(0);
    // The topic goes into the buffer's copy of the story, and the buffer is
    // written to the document the way every keystroke is — so the edit sits
    // inside the editing session's undo entry, as the marker does.
    if let Some((_, buffer)) = state.active_mut().editing.as_mut()
        && let Some(entry) = buffer.story_mut().index_entries.get_mut(index)
    {
        entry.topic = topic.trim().to_owned();
    }
    let Some(updated) = state
        .active()
        .editing
        .as_ref()
        .map(|(_, buffer)| buffer.story().clone())
    else {
        return false;
    };
    // undo-bracketed: the editing session recorded its entry when it began.
    state
        .active_mut()
        .document_mut()
        .replace_story_from_edit(story, updated);
    // undo-bracketed: same gesture.
    state.active_mut().document_mut().touch();
    state.active_mut().dirty = true;
    true
}

fn style_combo(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    chosen: &mut Option<ParagraphStyleId>,
    styles: &[(ParagraphStyleId, String)],
    none: &str,
) {
    let shown = chosen
        .and_then(|c| styles.iter().find(|(id, _)| *id == c))
        .map_or(none, |(_, name)| name.as_str());
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, None, none);
            for (id, name) in styles {
                ui.selectable_value(chosen, Some(*id), name);
            }
        });
}

fn contents(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.contents.open {
        return;
    }
    let mut window = state.contents.clone();
    let draft = window
        .draft
        .get_or_insert_with(|| state.active().document().contents.clone());
    let styles: Vec<(ParagraphStyleId, String)> = state
        .active()
        .document()
        .paragraph_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();
    let placed = draft
        .story
        .is_some_and(|s| state.active().document().story(s).is_some());

    let mut go = false;
    let mut remove: Option<usize> = None;
    let response = egui::Modal::new(egui::Id::new("table-of-contents"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(360.0, 520.0));
            ui.heading("Table of contents");
            ui.add_space(Theme::space_2());
            crate::view::panels::field(ui, "Title", |ui| {
                ui.add(egui::TextEdit::singleline(&mut draft.title).desired_width(f32::INFINITY));
            });
            crate::view::panels::field(ui, "Title style", |ui| {
                style_combo(
                    ui,
                    "toc-title-style",
                    &mut draft.title_style,
                    &styles,
                    "[Basic Paragraph]",
                );
            });
            ui.add_space(Theme::space_1());
            ui.label("Include paragraphs in these styles:");
            if draft.levels.is_empty() {
                ui.colored_label(Theme::text_muted(), "None yet — nothing would be listed.");
            }
            for (i, level) in draft.levels.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let mut style = Some(level.style);
                    style_combo(ui, ("toc-level", i), &mut style, &styles, "(choose)");
                    if let Some(style) = style {
                        level.style = style;
                    }
                    ui.label("set as");
                    style_combo(
                        ui,
                        ("toc-entry", i),
                        &mut level.entry_style,
                        &styles,
                        "[Basic Paragraph]",
                    );
                    if ui.button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if ui
                .add_enabled(!styles.is_empty(), egui::Button::new("Add a style"))
                .clicked()
                && let Some((style, _)) = styles.first()
            {
                draft.levels.push(Level {
                    style: *style,
                    entry_style: None,
                });
            }

            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                let verb = if placed {
                    "Update"
                } else {
                    "Place on this page"
                };
                go = ui
                    .add_enabled(!draft.levels.is_empty(), super::primary_button(verb))
                    .clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });
    if let (Some(at), Some(draft)) = (remove, window.draft.as_mut())
        && at < draft.levels.len()
    {
        draft.levels.remove(at);
    }
    if response.should_close() {
        window.open = false;
    }
    if go {
        let contents = window.draft.take().unwrap_or_default();
        apply(state, Command::SetContents(contents));
        apply(state, Command::UpdateContents);
        window.open = false;
    }
    if !window.open {
        window.draft = None;
    }
    state.contents = window;
}

fn endnotes(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.endnotes.open {
        return;
    }
    let mut window = state.endnotes.clone();
    let draft = window
        .draft
        .get_or_insert_with(|| state.active().document().endnotes.clone());
    let placed = draft
        .story
        .is_some_and(|s| state.active().document().story(s).is_some());
    let at_end = state.active().document().footnotes.placement
        == tessera_document::footnotes::NotePlacement::End;
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("endnotes"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 420.0));
            ui.heading("Endnotes");
            ui.add_space(Theme::space_2());
            crate::view::panels::field(ui, "Title", |ui| {
                ui.add(egui::TextEdit::singleline(&mut draft.title).desired_width(f32::INFINITY));
            });
            ui.colored_label(
                Theme::text_muted(),
                "Every note in the document, story by story, numbered as its reference is.",
            );
            if !at_end {
                // Said here, where the list is made, rather than left for
                // the person to find both the list and the notes at the foot.
                ui.colored_label(
                    Theme::error(),
                    "The footnote options still set the notes at the foot of the column: \
                     choose End of document there, or they will be set in both places.",
                );
            }
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                let verb = if placed {
                    "Update"
                } else {
                    "Place on this page"
                };
                go = ui.add(super::primary_button(verb)).clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });
    if response.should_close() {
        window.open = false;
    }
    if go {
        let endnotes = window.draft.take().unwrap_or_default();
        apply(state, Command::SetEndnotes(endnotes));
        apply(state, Command::UpdateEndnotes);
        window.open = false;
    }
    if !window.open {
        window.draft = None;
    }
    state.endnotes = window;
}

fn index(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.index.open {
        return;
    }
    let mut window = state.index.clone();
    let draft = window
        .draft
        .get_or_insert_with(|| state.active().document().index.clone());
    let placed = draft
        .story
        .is_some_and(|s| state.active().document().story(s).is_some());
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("index"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 420.0));
            ui.heading("Index");
            ui.add_space(Theme::space_2());
            crate::view::panels::field(ui, "Title", |ui| {
                ui.add(egui::TextEdit::singleline(&mut draft.title).desired_width(f32::INFINITY));
            });
            ui.colored_label(
                Theme::text_muted(),
                "Every index entry in the document, by topic, with its pages.",
            );
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                let verb = if placed {
                    "Update"
                } else {
                    "Place on this page"
                };
                go = ui.add(super::primary_button(verb)).clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });
    if response.should_close() {
        window.open = false;
    }
    if go {
        let index = window.draft.take().unwrap_or_default();
        apply(state, Command::SetIndex(index));
        apply(state, Command::UpdateIndex);
        window.open = false;
    }
    if !window.open {
        window.draft = None;
    }
    state.index = window;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{self, Run};
    use tessera_document::nodes::FrameKind;
    use tessera_geometry::DocRect;

    /// A document with one text frame saying `text`, being edited with the
    /// caret at its end.
    fn editing(text: &str) -> (TesseraApp, tessera_document::ids::FrameId) {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 20.0,
                y: 20.0,
                width: 300.0,
                height: 300.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: text.to_string(),
            },
        );
        crate::view::viewport::start_editing(&mut state, id);
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.set_cursor(text.len());
        }
        (state, id)
    }

    fn story_of(state: &TesseraApp, id: tessera_document::ids::FrameId) -> tessera_text::Story {
        let FrameKind::Text { story, .. } = state.active().document().frame(id).unwrap().kind
        else {
            panic!("a text frame");
        };
        state.active().document().story(story).unwrap().clone()
    }

    #[test]
    fn inserting_a_footnote_makes_a_note_and_opens_the_box_on_it() {
        let (mut state, id) = editing("A claim");
        actions::run(&mut state, Run::InsertFootnote);
        let story = story_of(&state, id);
        assert_eq!(story.footnotes.len(), 1, "one reference, one note");
        assert!(story.notes_are_sound());
        assert!(state.footnote.open, "the box opened on it");
        assert_eq!(state.footnote.index, 0);
        assert_eq!(state.footnote.text, "", "a fresh note has no words yet");

        // Wording it goes through the command and keeps the number.
        let note_story = state.footnote.story.unwrap();
        apply(
            &mut state,
            Command::SetFootnoteText {
                story: note_story,
                index: 0,
                text: "The source.".into(),
            },
        );
        let story = story_of(&state, id);
        let (prefix, rest) = split_prefix(&story.footnotes[0].text);
        assert!(!prefix.is_empty(), "the number marker stays");
        assert_eq!(rest, "The source.");
        // And the buffer agrees, so the next keystroke does not undo it.
        let buffer = &state.active().editing.as_ref().unwrap().1;
        assert_eq!(
            split_prefix(&buffer.story().footnotes[0].text).1,
            "The source."
        );
    }

    #[test]
    fn an_index_entry_goes_in_front_of_the_selection_and_keeps_it() {
        let (mut state, id) = editing("Caslon set type");
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.select(0..6);
        }
        assert!(insert_index_entry(&mut state, "Caslon, William"));
        let story = story_of(&state, id);
        assert!(story.text.starts_with(Marker::IndexEntry.character()));
        assert!(
            story.text.ends_with("Caslon set type"),
            "nothing was replaced"
        );
        assert_eq!(story.index_entries[0].topic, "Caslon, William");
    }

    #[test]
    fn placing_the_contents_makes_a_frame_and_updating_rewrites_it() {
        use tessera_document::contents::{Contents, Level};
        use tessera_text::story::{ParagraphFormat, ParagraphStyle};

        let (mut state, id) = editing("Alpha\nbody");
        state.active_mut().editing = None;
        let heading = state
            .active_mut()
            .document_mut()
            .add_paragraph_style(ParagraphStyle {
                name: "Heading".into(),
                based_on: None,
                format: ParagraphFormat::default(),
            });
        let FrameKind::Text { story, .. } = state.active().document().frame(id).unwrap().kind
        else {
            panic!()
        };
        state
            .active_mut()
            .document_mut()
            .story_mut(story)
            .unwrap()
            .set_paragraph_style(0..6, Some(heading));
        apply(
            &mut state,
            Command::SetContents(Contents {
                title: "Contents".into(),
                title_style: None,
                levels: vec![Level {
                    style: heading,
                    entry_style: None,
                }],
                story: None,
            }),
        );
        let frames_before = state.active().document().paint_order().len();
        apply(&mut state, Command::UpdateContents);
        let doc = state.active().document();
        assert_eq!(
            doc.paint_order().len(),
            frames_before + 1,
            "a frame was placed"
        );
        let toc = doc.contents.story.expect("recorded where it went");
        assert_eq!(doc.story(toc).unwrap().text, "Contents\nAlpha\t1");

        // A second heading, then update: same frame, new words.
        state
            .active_mut()
            .document_mut()
            .story_mut(story)
            .unwrap()
            .insert_text(10, "\nBeta");
        state
            .active_mut()
            .document_mut()
            .story_mut(story)
            .unwrap()
            .set_paragraph_style(11..15, Some(heading));
        state.active_mut().document_mut().touch();
        apply(&mut state, Command::UpdateContents);
        let doc = state.active().document();
        assert_eq!(
            doc.paint_order().len(),
            frames_before + 1,
            "no second frame"
        );
        assert_eq!(doc.story(toc).unwrap().text, "Contents\nAlpha\t1\nBeta\t1");
    }

    #[test]
    fn the_prefix_is_the_number_and_its_tab_and_nothing_else() {
        let fresh = tessera_text::Story::new_footnote();
        let (prefix, rest) = split_prefix(&fresh.text);
        assert_eq!(prefix, fresh.text);
        assert_eq!(rest, "");
        let (prefix, rest) = split_prefix("plain words");
        assert_eq!(prefix, "");
        assert_eq!(rest, "plain words");
    }
}
