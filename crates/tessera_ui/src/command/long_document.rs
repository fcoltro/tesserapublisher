//! What a long document generates and numbers: contents, index, endnotes, sections, chapters, and data merge.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::SetSections(sections) => {
            state.active_mut().document_mut().set_sections(sections);
        }

        Command::SetChapter(chapter) => {
            state.active_mut().document_mut().set_chapter(chapter);
        }

        Command::SetDataSource(source) => {
            state.active_mut().document_mut().set_data_source(source);
        }

        Command::SetMergePicture { frame, field } => {
            let Some(mut source) = state.active().document().data_merge.clone() else {
                return;
            };
            source.set_picture(frame, field);
            state
                .active_mut()
                .document_mut()
                .set_data_source(Some(source));
        }

        Command::SetContents(contents) => {
            state.active_mut().document_mut().set_contents(contents);
        }

        Command::UpdateContents => {
            // Built from the layout as it stands, so the resolve comes first
            // and the write after; the frame the contents go into is the one
            // they were placed in, measured for the right tab.
            let contents = state.active().document().contents.clone();
            let resolved = state.resolve_active().clone();
            let measure = contents_measure(state, &contents);
            let doc = state.active().document();
            let generated = tessera_layout::contents::table_of_contents(
                doc,
                &resolved,
                &contents.title,
                contents.title_style,
                &contents.levels,
                measure,
            );
            place_contents(state, generated.story, generated.destinations);
        }

        Command::PlaceContents {
            story,
            destinations,
        } => place_contents(state, story, destinations),

        Command::SetIndex(index) => {
            state.active_mut().document_mut().set_index(index);
        }

        Command::UpdateIndex => {
            let index = state.active().document().index.clone();
            let resolved = state.resolve_active().clone();
            let story =
                tessera_layout::contents::index(state.active().document(), &resolved, &index.title);
            place_generated(state, story, index.story, |doc, id| {
                doc.index.story = Some(id);
            });
        }

        Command::PlaceIndex { story } => {
            let at = state.active().document().index.story;
            place_generated(state, story, at, |doc, id| {
                doc.index.story = Some(id);
            });
        }

        Command::SetEndnotes(endnotes) => {
            state.active_mut().document_mut().set_endnotes(endnotes);
        }

        Command::UpdateEndnotes => {
            let endnotes = state.active().document().endnotes.clone();
            let resolved = state.resolve_active().clone();
            let story = tessera_layout::contents::endnotes(
                state.active().document(),
                &resolved,
                &endnotes.title,
            );
            place_generated(state, story, endnotes.story, |doc, id| {
                doc.endnotes.story = Some(id);
            });
        }
        _ => unreachable!("not a command for long_document"),
    }
}
