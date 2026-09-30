//! Text: its words, its formatting, its frames and threads, and the markers set in it.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::FitFrameToText { id } => {
            let key = state.active;
            let fitted = tessera_layout::resolve::height_to_fit(
                state.documents[key].document(),
                &mut state.shaper,
                id,
            );
            let Some(height) = fitted else {
                state.status = Some(crate::app::Status::info(
                    "this frame cannot be fitted to its text: it passes text on, \
                     or would be taller than any page",
                ));
                return;
            };
            let Some(mut bounds) = state.active().document().frame(id).map(|f| f.bounds) else {
                return;
            };
            if bounds.height != height {
                bounds.height = height;
                let placement = state
                    .active()
                    .document()
                    .frame(id)
                    .map_or(Transform::IDENTITY, |f| f.transform);
                retarget(state, id, bounds, placement);
            }
        }

        Command::ReplaceFamily { from, to } => {
            state.active_mut().document_mut().replace_family(&from, &to);
        }

        Command::SetText { id, text } => {
            if let Some(FrameKind::Text { story, .. }) =
                state.active().document().frame(id).map(|f| f.kind.clone())
                && let Some(mut s) = state.active().document().story(story).cloned()
            {
                // Not `s.text = text`. Assigning the string leaves `runs`
                // describing a length the text no longer has, which is
                // corruption rather than a glitch and shows up far from here.
                s.set_text(text);
                // End the matching session before it can write its old copy back.
                if editing_buffer_for(state, story).is_some() {
                    state.active_mut().editing = None;
                    state.active_mut().editing_cell = None;
                    state.active_mut().editing_note = None;
                }
                state
                    .active_mut()
                    .document_mut()
                    .replace_story_from_edit(story, s);
            }
        }

        Command::ReplaceMatches { edits } => {
            for (story, range, with) in edits {
                let Some(mut s) = state.active().document().story(story).cloned() else {
                    continue;
                };
                // A range that no longer fits is one the document changed
                // under the search. Skipped rather than clamped: a clamped
                // range would edit text nobody looked for.
                if range.start > range.end
                    || range.end > s.text.len()
                    || !s.text.is_char_boundary(range.start)
                    || !s.text.is_char_boundary(range.end)
                {
                    continue;
                }
                // Through the story's own operations, which carry the run
                // table with them. Assigning the string would leave `runs`
                // describing a length the text no longer has.
                s.delete_range(range.clone());
                s.insert_text(range.start, &with);
                // Protect every caller, including the generic command bridge.
                // A canvas buffer must never write its pre-replacement copy back.
                if editing_buffer_for(state, story).is_some() {
                    state.active_mut().editing = None;
                    state.active_mut().editing_cell = None;
                    state.active_mut().editing_note = None;
                }
                // As an edit, so a marker replaced away takes its anchored
                // frame with it rather than leaving it pointing at nothing.
                state
                    .active_mut()
                    .document_mut()
                    .replace_story_from_edit(story, s);
            }
        }

        Command::SetCharacterFormat {
            story,
            range,
            format,
        } => {
            // An empty range is a caret, and character formatting needs
            // something to sit on. Held for the next text typed rather than
            // discarded — discarding is what used to happen, and it looked
            // exactly like a broken control: the picker moved and the page did
            // not.
            if range.start >= range.end {
                if let Some(buffer) = editing_buffer_for(state, story) {
                    buffer.set_pending(&format);
                }
                return;
            }

            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.apply_character_format(range.clone(), &format);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.apply_character_format(range, &format);
            }
        }

        Command::SetParagraphFormat {
            story,
            range,
            format,
        } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.apply_paragraph_format(range.clone(), &format);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.apply_paragraph_format(range, &format);
            }
        }

        Command::SetTextLayout { id, layout } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id)
                && let FrameKind::Text { story, .. } = frame.kind
            {
                frame.kind = FrameKind::Text { story, layout };
            }
            state.active_mut().document_mut().touch();
        }

        Command::ThreadFrames { from, to } => {
            let doc = state.active_mut().document_mut();
            // From a table: on into the frame, as its next part.
            if doc.table_behind(from).is_some() {
                doc.continue_table_into(from, to);
            } else {
                doc.thread(from, to);
            }
        }

        Command::UnthreadFrame { id } => {
            let doc = state.active_mut().document_mut();
            if doc.table_behind(id).is_some() {
                doc.stop_table_at(id);
            } else {
                doc.unthread(id);
            }
        }

        Command::PutTextOnPath { id, text } => {
            let doc = state.active_mut().document_mut();
            let is_path = matches!(doc.frame(id).map(|f| &f.kind), Some(FrameKind::Path(_)));
            if is_path && doc.path_text(id).is_none() {
                let story = doc.add_story(Story::new(&text));
                doc.set_path_text(id, Some(tessera_document::path_text::PathText::new(story)));
            }
        }

        Command::SetPathText { id, text } => {
            state.active_mut().document_mut().set_path_text(id, text);
        }

        Command::SetVariables(variables) => {
            state.active_mut().document_mut().set_variables(variables);
        }

        Command::SetTextAnchor { story, index, name } => {
            if let Some(anchor) = state
                .active_mut()
                .document_mut()
                .story_mut(story)
                .and_then(|s| s.anchors.get_mut(index))
            {
                anchor.name = name.trim().to_owned();
            }
            state.active_mut().document_mut().touch();
            if let Some(buffer) = editing_buffer_for(state, story)
                && let Some(anchor) = buffer.story_mut().anchors.get_mut(index)
            {
                anchor.name = name.trim().to_owned();
            }
        }

        Command::SetCrossReference {
            story,
            index,
            reference,
        } => {
            if let Some(slot) = state
                .active_mut()
                .document_mut()
                .story_mut(story)
                .and_then(|s| s.cross_references.get_mut(index))
            {
                *slot = reference.clone();
            }
            state.active_mut().document_mut().touch();
            if let Some(buffer) = editing_buffer_for(state, story)
                && let Some(slot) = buffer.story_mut().cross_references.get_mut(index)
            {
                *slot = reference;
            }
        }

        Command::SetFootnoteText { story, index, text } => {
            if let Some(note) = state
                .active_mut()
                .document_mut()
                .story_mut(story)
                .and_then(|s| s.footnotes.get_mut(index))
            {
                let prefix = crate::view::long_document::split_prefix(&note.text)
                    .0
                    .to_owned();
                note.set_text(format!("{prefix}{text}"));
            }
            state.active_mut().document_mut().touch();
            // The buffer holds its own copy of the story it is editing.
            if let Some((id, buffer)) = state.active_mut().editing.as_mut()
                && let Some(note) = buffer.story_mut().footnotes.get_mut(index)
            {
                let _ = id;
                let prefix = crate::view::long_document::split_prefix(&note.text)
                    .0
                    .to_owned();
                note.set_text(format!("{prefix}{text}"));
            }
        }

        Command::SetDestination { name, page } => {
            state
                .active_mut()
                .document_mut()
                .set_destination(name, page);
        }

        Command::SetFootnoteOptions(options) => {
            state
                .active_mut()
                .document_mut()
                .set_footnote_options(options);
        }

        Command::PasteAnchored => {
            use tessera_document::anchored::{Anchored, MARKER};
            let Some(first) = state.clipboard.first() else {
                return;
            };
            let Some((frame, buffer)) = state.active().editing.as_ref() else {
                return;
            };
            let (frame, at) = (*frame, buffer.cursor().position);
            let Some(story) =
                crate::view::viewport::editing_story(state, frame, state.active().editing_cell)
            else {
                return;
            };
            let source = first.source.clone();
            let root = first.root;
            let layer = state.default_layer();
            let pasted = state.active_mut().document_mut().import_frames(
                &source,
                &[root],
                layer,
                0.0,
                0.0,
                false,
            );
            let pasted = match pasted {
                Ok(pasted) => pasted,
                Err(message) => {
                    state.status = Some(crate::app::Status::error(message));
                    return;
                }
            };
            let Some(id) = pasted.first().copied() else {
                return;
            };
            // Which marker this will be: the ones before the caret come first.
            let index = state
                .active()
                .document()
                .story(story)
                .map(|s| {
                    tessera_document::anchored::marker_offsets(&s.text)
                        .iter()
                        .filter(|m| **m < at)
                        .count()
                })
                .unwrap_or(0);
            // The marker goes into the buffer and is written back the way a
            // keystroke is, which renumbers the anchors after it; then this
            // frame takes the index the new marker has.
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.set_cursor(at);
                buffer.insert(&MARKER.to_string());
                let edited = buffer.story().clone();
                state
                    .active_mut()
                    .document_mut()
                    .replace_story_from_edit(story, edited);
            } else if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.insert_text(at, &MARKER.to_string());
            }
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.anchor = Some(Anchored::new(story, index));
            }
            state.active_mut().document_mut().touch();
            state.active_mut().selection.set(id);
        }

        Command::PlaceText { id, text } => {
            let PlacedText {
                mut story,
                paragraph_styles,
                character_styles,
                paragraph_style_names,
                run_style_names,
            } = text;
            let doc = state.active_mut().document_mut();
            let mut paragraph_ids = std::collections::HashMap::new();
            for style in paragraph_styles {
                let existing = doc
                    .paragraph_styles
                    .iter()
                    .find(|(_, s)| s.name == style.name)
                    .map(|(id, _)| id);
                let name = style.name.clone();
                let id = existing.unwrap_or_else(|| doc.add_paragraph_style(style));
                paragraph_ids.insert(name, id);
            }
            let mut character_ids = std::collections::HashMap::new();
            for style in character_styles {
                let existing = doc
                    .character_styles
                    .iter()
                    .find(|(_, s)| s.name == style.name)
                    .map(|(id, _)| id);
                let name = style.name.clone();
                let id = existing.unwrap_or_else(|| doc.add_character_style(style));
                character_ids.insert(name, id);
            }
            for (paragraph, name) in story.paragraphs.iter_mut().zip(&paragraph_style_names) {
                paragraph.style = name.as_ref().and_then(|n| paragraph_ids.get(n)).copied();
            }
            for (run, name) in story.runs.iter_mut().zip(&run_style_names) {
                run.style = name.as_ref().and_then(|n| character_ids.get(n)).copied();
            }
            let into =
                id.and_then(
                    |id| match state.active().document().frame(id).map(|f| &f.kind) {
                        Some(FrameKind::Text { story, .. }) => Some(*story),
                        _ => None,
                    },
                );
            place_generated(state, story, into, |_, _| {});
            // What placing a manuscript as body text means is setting all of
            // it: the thread carries on onto as many pages as it needs, in the
            // same undo step. Placed into a box of somebody's own, it stays
            // there, overset, as the box was chosen for it.
            let placed = id.or_else(|| state.active().selection.single());
            if state.prefs.flow_placed_text
                && let Some(frame) = placed
                && crate::reflow::fills_margins(state.active().document(), frame)
            {
                flow_text(state, frame);
            }
        }

        Command::FlowText { id } => {
            flow_text(state, id);
        }
        _ => unreachable!("not a command for text"),
    }
}
