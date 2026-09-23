//! Text anchors and cross-references: "see Chapter Two on page 12", kept
//! true as the pages move.
//!
//! An anchor is a named marker in the text; a cross-reference is a marker
//! that reads as where its target is — the page, the paragraph, or both —
//! worked out by the layout every time it lays the pages out. Both are
//! inserted at the caret the way an index entry is: the marker is typed
//! into the editing buffer, its entry is filled in the buffer's copy of the
//! story, and the buffer is written back, so the whole thing sits inside
//! the editing session's undo entry.

use tessera_text::story::{CrossReference, CrossReferenceFormat};
use tessera_text::variables::Marker;

use crate::app::TesseraApp;
use crate::theme::Theme;

/// The box for a new anchor: its name.
#[derive(Debug, Clone, Default)]
pub struct TextAnchorWindow {
    pub open: bool,
    pub name: String,
}

/// The box for a new cross-reference: what it points at, and how it reads.
#[derive(Debug, Clone, Default)]
pub struct CrossReferenceWindow {
    pub open: bool,
    pub target: String,
    pub format: CrossReferenceFormat,
}

/// Every name a cross-reference may point at: the text anchors in every
/// story, then the named page destinations.
pub fn targets(state: &TesseraApp) -> Vec<String> {
    let doc = state.active().document();
    let mut names: Vec<String> = doc
        .stories
        .values()
        .flat_map(|s| s.anchors.iter().map(|a| a.name.clone()))
        .filter(|n| !n.trim().is_empty())
        .collect();
    names.extend(doc.destinations.iter().map(|d| d.name.clone()));
    names.sort();
    names.dedup();
    names
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    anchor_box(ctx, state);
    reference_box(ctx, state);
}

fn anchor_box(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.text_anchor.open {
        return;
    }
    let mut window = state.text_anchor.clone();
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("text-anchor"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 420.0));
            ui.heading("Text anchor");
            ui.colored_label(
                Theme::text_muted(),
                "A named place in the text a cross-reference can point at.",
            );
            ui.add_space(Theme::space_2());
            crate::view::panels::field(ui, "Name", |ui| {
                ui.add(egui::TextEdit::singleline(&mut window.name).desired_width(f32::INFINITY));
            });
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                go = ui
                    .add_enabled(
                        !window.name.trim().is_empty(),
                        super::primary_button("Insert"),
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
    state.text_anchor = window;
    if go {
        let name = state.text_anchor.name.trim().to_owned();
        insert_anchor(state, &name);
        state.text_anchor.open = false;
    }
}

fn reference_box(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.cross_reference.open {
        return;
    }
    let mut window = state.cross_reference.clone();
    let mut go = false;
    let targets = targets(state);
    let response = egui::Modal::new(egui::Id::new("cross-reference"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 440.0));
            ui.heading("Cross-reference");
            ui.add_space(Theme::space_2());
            crate::view::panels::field(ui, "To", |ui| {
                if targets.is_empty() {
                    ui.colored_label(
                        Theme::text_muted(),
                        "No anchors yet. Type › Insert marker › Text anchor puts one in the text.",
                    );
                } else {
                    crate::icons::reads_as(
                        egui::ComboBox::from_id_salt("cross-reference-target")
                            .selected_text(if window.target.is_empty() {
                                "Choose…"
                            } else {
                                window.target.as_str()
                            })
                            .width(ui.available_width())
                            .show_ui(ui, |ui| {
                                for name in &targets {
                                    ui.selectable_value(&mut window.target, name.clone(), name);
                                }
                            })
                            .response,
                        "To",
                        egui::WidgetType::ComboBox,
                        None,
                    );
                }
            });
            crate::view::panels::field(ui, "Reads as", |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (format, label) in [
                        (CrossReferenceFormat::PageNumber, "Page number"),
                        (CrossReferenceFormat::ParagraphText, "Paragraph text"),
                        (CrossReferenceFormat::ParagraphAndPage, "Paragraph and page"),
                    ] {
                        ui.selectable_value(&mut window.format, format, label);
                    }
                });
            });
            ui.colored_label(
                Theme::text_muted(),
                match window.format {
                    CrossReferenceFormat::PageNumber => "\u{201c}12\u{201d}",
                    CrossReferenceFormat::ParagraphText => "\u{201c}Chapter Two\u{201d}",
                    CrossReferenceFormat::ParagraphAndPage => {
                        "\u{201c}Chapter Two on page 12\u{201d}"
                    }
                },
            );
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                go = ui
                    .add_enabled(!window.target.is_empty(), super::primary_button("Insert"))
                    .clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });
    if response.should_close() {
        window.open = false;
    }
    state.cross_reference = window;
    if go {
        let reference = CrossReference {
            target: state.cross_reference.target.clone(),
            format: state.cross_reference.format,
        };
        insert_reference(state, reference);
        state.cross_reference.open = false;
    }
}

/// Put an anchor marker at the caret, called `name`. At the *start* of a
/// selection, without replacing it, as an index entry is.
pub(crate) fn insert_anchor(state: &mut TesseraApp, name: &str) -> bool {
    insert_marker(state, Marker::TextAnchor, |story, index| {
        if let Some(anchor) = story.anchors.get_mut(index) {
            anchor.name = name.trim().to_owned();
        }
    })
}

/// Put a cross-reference marker at the caret, replacing the selection —
/// a reference is text, and typing over a selection is what typing does.
pub(crate) fn insert_reference(state: &mut TesseraApp, reference: CrossReference) -> bool {
    insert_marker(state, Marker::CrossReference, |story, index| {
        if let Some(slot) = story.cross_references.get_mut(index) {
            *slot = reference.clone();
        }
    })
}

fn insert_marker(
    state: &mut TesseraApp,
    marker: Marker,
    fill: impl Fn(&mut tessera_text::story::Story, usize),
) -> bool {
    let Some((id, buffer)) = state.active_mut().editing.as_mut() else {
        return false;
    };
    let id = *id;
    if marker == Marker::TextAnchor
        && let Some(range) = buffer.selection_range()
    {
        buffer.set_cursor(range.start);
    }
    let at = buffer
        .selection_range()
        .map_or(buffer.cursor().position, |r| r.start);
    if !crate::view::viewport::type_text(state, &marker.character().to_string()) {
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
        .map(|s| match marker {
            Marker::TextAnchor => s.anchor_at(at),
            _ => s.cross_reference_at(at),
        })
        .unwrap_or(0);
    if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
        fill(buffer.story_mut(), index);
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
    state.active_mut().document_mut().touch();
    state.active_mut().dirty = true;
    true
}

/// A marker at the caret, if the caret stands right after one: what to
/// offer to edit.
pub fn reference_at_caret(state: &TesseraApp) -> Option<(tessera_document::ids::StoryId, usize)> {
    let (id, buffer) = state.active().editing.as_ref()?;
    let story_id = crate::view::viewport::editing_story(state, *id, state.active().editing_cell)?;
    let story = buffer.story();
    let at = buffer.cursor().position;
    let width = Marker::CrossReference.character().len_utf8();
    let before = at.checked_sub(width)?;
    // `get`, not a slice: three bytes back from a caret after two accented
    // letters is inside the first of them.
    if Marker::of(story.text.get(before..)?.chars().next()?) == Some(Marker::CrossReference) {
        Some((story_id, story.cross_reference_at(before)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply};
    use tessera_document::nodes::FrameKind;
    use tessera_geometry::DocRect;

    fn editing(text: &str) -> (TesseraApp, tessera_document::ids::StoryId) {
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
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: text.to_string(),
            },
        );
        let FrameKind::Text { story, .. } = state.active().document().frame(id).unwrap().kind
        else {
            panic!()
        };
        crate::view::viewport::start_editing(&mut state, id);
        (state, story)
    }

    #[test]
    fn an_anchor_and_a_reference_are_inserted_at_the_caret_with_their_entries() {
        let (mut state, story) = editing("Chapter Two\nSee  for more.");
        // The anchor at the head of the chapter.
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.set_cursor(0);
        }
        assert!(insert_anchor(&mut state, "ch2"));
        let s = state.active().document().story(story).unwrap();
        assert_eq!(s.anchors.len(), 1);
        assert_eq!(s.anchors[0].name, "ch2");
        assert!(s.text.starts_with(Marker::TextAnchor.character()));

        // The reference after "See ".
        let at = s.text.find("See ").unwrap() + 4;
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.set_cursor(at);
        }
        assert!(insert_reference(
            &mut state,
            CrossReference {
                target: "ch2".into(),
                format: CrossReferenceFormat::ParagraphAndPage,
            }
        ));
        let s = state.active().document().story(story).unwrap();
        assert_eq!(s.cross_references.len(), 1);
        assert_eq!(s.cross_references[0].target, "ch2");
        assert!(s.notes_are_sound());
        assert_eq!(targets(&state), vec!["ch2".to_string()]);
        // The caret sits after the reference, which is what to offer to edit.
        assert_eq!(reference_at_caret(&state), Some((story, 0)));
    }

    #[test]
    fn a_caret_after_accented_letters_offers_nothing_rather_than_crashing() {
        // Three bytes back from the caret after "éé" is inside the first é;
        // slicing there used to panic.
        let (mut state, _) = editing("éé");
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.set_cursor(4);
        }
        assert_eq!(reference_at_caret(&state), None);
    }

    #[test]
    fn nothing_is_inserted_when_nothing_is_being_edited() {
        let mut state = TesseraApp::headless();
        assert!(!insert_anchor(&mut state, "x"));
        assert!(targets(&state).is_empty());
    }
}
