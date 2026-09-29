//! Paragraph and character styles, and a book's styles synchronised.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::DefineCharacterStyle(style) => {
            state.active_mut().document_mut().add_character_style(style);
        }

        Command::DefineParagraphStyle(style) => {
            state.active_mut().document_mut().add_paragraph_style(style);
        }

        Command::EditCharacterStyle { id, style } => {
            if let Some(existing) = state.active_mut().document_mut().character_style_mut(id) {
                *existing = style;
            }
        }

        Command::EditParagraphStyle { id, style } => {
            if let Some(existing) = state.active_mut().document_mut().paragraph_style_mut(id) {
                *existing = style;
            }
        }

        Command::ClearCharacterOverrides { story, range } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.clear_character_overrides(range.clone());
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.clear_character_overrides(range);
            }
        }

        Command::ClearParagraphOverrides { story, range } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.clear_paragraph_overrides(range.clone());
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.clear_paragraph_overrides(range);
            }
        }

        Command::RedefineCharacterStyle { id, story, range } => {
            // What the text says over and above its style, read before
            // anything moves.
            let Some(overrides) = state
                .active()
                .document()
                .story(story)
                .map(|s| s.common_format_local(range.clone()))
            else {
                return;
            };
            if let Some(style) = state.active_mut().document_mut().character_style_mut(id) {
                style.format = overrides.over(&style.format);
            }
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.clear_character_overrides(range.clone());
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.clear_character_overrides(range);
            }
        }

        Command::RedefineParagraphStyle { id, story, range } => {
            let Some((paragraph, character)) = state.active().document().story(story).map(|s| {
                (
                    s.common_paragraph_format(range.clone()),
                    s.common_format_local(range.clone()),
                )
            }) else {
                return;
            };
            if let Some(style) = state.active_mut().document_mut().paragraph_style_mut(id) {
                let mut wanted = paragraph;
                wanted.character = character.over(&wanted.character);
                style.format = wanted.over(&style.format);
            }
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.clear_paragraph_overrides(range.clone());
                s.clear_character_overrides(range.clone());
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.clear_paragraph_overrides(range.clone());
                buffer.clear_character_overrides(range);
            }
        }

        Command::BreakCharacterStyleLink { story, range } => {
            // The style's resolved format has to be read before the link goes,
            // and it is the *chain* rather than the one style: a child style
            // whose parent supplied the family would otherwise lose it.
            let Some((id, format)) = state.active().document().story(story).and_then(|s| {
                let (id, _) = s.common_character_style(range.clone());
                id.map(|id| (id, state.active().document().character_chain(id)))
            }) else {
                return;
            };
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.clear_character_style_link(range.clone(), id, &format);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.clear_character_style_link(range, id, &format);
            }
        }

        Command::BreakParagraphStyleLink { story, range } => {
            let Some((id, format)) = state.active().document().story(story).and_then(|s| {
                let (id, _) = s.common_paragraph_style(range.clone());
                id.map(|id| (id, state.active().document().paragraph_chain(id)))
            }) else {
                return;
            };
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.clear_paragraph_style_link(range.clone(), id, &format);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.clear_paragraph_style_link(range, id, &format);
            }
        }

        Command::SetCharacterStyleBasedOn { id, based_on } => {
            // A cycle is refused here as well as hidden from the picker: the
            // command is public and undo replays it.
            if let Some(parent) = based_on
                && (parent == id
                    || state
                        .active()
                        .document()
                        .character_based_on_would_cycle(id, parent))
            {
                return;
            }
            if let Some(style) = state.active_mut().document_mut().character_style_mut(id) {
                style.based_on = based_on;
            }
        }

        Command::SetParagraphStyleBasedOn { id, based_on } => {
            if let Some(parent) = based_on
                && (parent == id
                    || state
                        .active()
                        .document()
                        .paragraph_based_on_would_cycle(id, parent))
            {
                return;
            }
            if let Some(style) = state.active_mut().document_mut().paragraph_style_mut(id) {
                style.based_on = based_on;
            }
        }

        Command::DeleteCharacterStyle { id } => {
            // The format has to be read before the style goes, and every story
            // folded before anything is removed — a story left referring to a
            // deleted style silently loses its formatting, because
            // `resolve_run` treats an unknown id as saying nothing.
            let Some(format) = state
                .active()
                .document()
                .character_styles
                .get(id)
                .map(|_| state.active().document().character_chain(id))
            else {
                return;
            };
            let ids: Vec<StoryId> = state.active().document().stories.keys().collect();
            for story in ids {
                if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                    s.flatten_character_style(id, &format);
                }
            }
            if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
                buffer.flatten_character_style(id, &format);
            }
            state.active_mut().document_mut().remove_character_style(id);
        }

        Command::DeleteParagraphStyle { id } => {
            let Some(format) = state
                .active()
                .document()
                .paragraph_styles
                .get(id)
                .map(|_| state.active().document().paragraph_chain(id))
            else {
                return;
            };
            let ids: Vec<StoryId> = state.active().document().stories.keys().collect();
            for story in ids {
                if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                    s.flatten_paragraph_style(id, &format);
                }
            }
            if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
                buffer.flatten_paragraph_style(id, &format);
            }
            state.active_mut().document_mut().remove_paragraph_style(id);
        }

        Command::SetCharacterStyleOf {
            story,
            range,
            style,
        } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.set_character_style(range.clone(), style);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.set_character_style(range, style);
            }
        }

        Command::SetParagraphStyleOf {
            story,
            range,
            style,
        } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.set_paragraph_style(range.clone(), style);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.set_paragraph_style(range, style);
            }
        }

        Command::SynchroniseStyles(sheet) => {
            state.active_mut().document_mut().synchronise_styles(&sheet);
        }
        _ => unreachable!("not a command for text_styles"),
    }
}
