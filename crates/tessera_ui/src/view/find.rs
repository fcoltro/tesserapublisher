//! The Find and Change window.
//!
//! **Modeless**, which is not a detail: finding a word is something a person
//! does *while* laying out, and a modal box that has to be dismissed before the
//! document can be touched turns every search into two extra gestures. The
//! window stays open, the canvas stays live, and the keyboard goes wherever the
//! focus is — which is what `keys_are_ours` in the viewport already arranges.
//!
//! The searching itself is in [`crate::find`]. This is only the surface.

use egui::Ui;

use crate::app::TesseraApp;
use crate::find::{self, Query};
use crate::theme::Theme;

#[derive(Default)]
pub struct FindWindow {
    pub open: bool,
    pub query: Query,
    pub replacement: String,
    /// Which hit of the last search we are standing on.
    ///
    /// Invalidated whenever the document, revision or query changes.
    pub at: Option<usize>,
    /// What the last action did, in words. `None` before anything is asked.
    pub note: Option<String>,
    /// Set when the window opens, so typing can start immediately.
    focus: bool,
    /// A result index only has meaning for this document, revision and query.
    context: Option<(crate::app::DocumentKey, u64, Query)>,
}

impl FindWindow {
    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
        self.note = None;
        self.at = None;
        self.context = None;
    }
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.find.open {
        return;
    }
    let mut open = true;
    egui::Window::new("Find and Change")
        .open(&mut open)
        .resizable(false)
        .default_width(420.0)
        .show(ctx, |ui| body(ui, state));
    if !open {
        state.find.open = false;
    }
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    ui.spacing_mut().item_spacing.y = Theme::space_2();
    ui.weak(format!("Search in {}", state.active().title()));

    let needle = egui::Grid::new("find-fields")
        .num_columns(2)
        .show(ui, |ui| {
            let label = ui.label("Find");
            let needle = ui
                .add(
                    egui::TextEdit::singleline(&mut state.find.query.needle)
                        .desired_width(f32::INFINITY)
                        .hint_text("text to find"),
                )
                .labelled_by(label.id);
            ui.end_row();
            let label = ui.label("Change to");
            ui.add(
                egui::TextEdit::singleline(&mut state.find.replacement)
                    .desired_width(f32::INFINITY)
                    .hint_text("leave empty to delete"),
            )
            .labelled_by(label.id);
            ui.end_row();
            needle
        })
        .inner;
    if std::mem::take(&mut state.find.focus) {
        needle.request_focus();
    }

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.find.query.match_case, "Match case");
        ui.checkbox(&mut state.find.query.whole_word, "Whole word");
    });
    sync_context(state);

    // Return in the find box is Find Next, which is what every search box in
    // every application does and what a person will try first.
    let entered = needle.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let backwards = ui.input(|i| i.modifiers.shift);
    if entered {
        needle.request_focus();
    }

    ui.separator();

    let runnable = state.find.query.is_runnable();
    let mut find_next = entered && !backwards;
    let mut find_previous = entered && backwards;
    let mut change = false;
    let mut change_all = false;

    ui.horizontal_wrapped(|ui| {
        find_previous |= ui
            .add_enabled(runnable, egui::Button::new("Previous"))
            .on_hover_text("Shift+Enter in the Find field")
            .clicked();
        find_next |= ui
            .add_enabled(runnable, super::primary_button("Find next"))
            .on_hover_text("Enter in the Find field")
            .clicked();
    });
    ui.horizontal_wrapped(|ui| {
        change |= ui
            .add_enabled(
                runnable && state.find.at.is_some(),
                egui::Button::new("Change"),
            )
            .on_disabled_hover_text("Find an occurrence before changing it")
            .clicked();
        change_all |= ui
            .add_enabled(runnable, egui::Button::new("Change all"))
            .on_hover_text(
                "Change every occurrence in this document. Undo reverses the whole change.",
            )
            .clicked();
    });

    if find_next {
        go_to_next(state);
    }
    if find_previous {
        go_to_previous(state);
    }
    if change {
        change_one(state);
    }
    if change_all {
        change_every(state);
    }

    if let Some(note) = &state.find.note {
        ui.colored_label(Theme::text_muted(), note);
    } else {
        ui.weak("Enter to find next · Shift+Enter to find previous");
    }
}

fn sync_context(state: &mut TesseraApp) {
    let context = (
        state.active,
        state.active().document().revision(),
        state.find.query.clone(),
    );
    if state.find.context.as_ref() != Some(&context) {
        state.find.at = None;
        state.find.note = None;
        state.find.context = Some(context);
    }
}

/// Move to the hit after the one we are on, wrapping at the end.
fn go_to_next(state: &mut TesseraApp) {
    navigate(state, false);
}

fn go_to_previous(state: &mut TesseraApp) {
    navigate(state, true);
}

fn navigate(state: &mut TesseraApp, backwards: bool) {
    sync_context(state);
    let hits = find::search(state.active().document(), &state.find.query);
    if hits.is_empty() {
        state.find.at = None;
        state.find.note = Some("Not found.".into());
        return;
    }
    // Wrapping is what a person expects and InDesign asks about; asking is a
    // dialog in front of a dialog, and stopping dead at the last hit means a
    // search started halfway down the document never sees the top of it.
    let next = match (state.find.at, backwards) {
        (Some(at), true) if at > 0 && at < hits.len() => at - 1,
        (_, true) => hits.len() - 1,
        (Some(at), false) if at + 1 < hits.len() => at + 1,
        _ => 0,
    };
    select_hit(state, &hits, next);
}

fn select_hit(state: &mut TesseraApp, hits: &[find::Hit], next: usize) {
    sync_context(state);
    state.find.at = Some(next);
    state.find.note = Some(format!("{} of {}", next + 1, hits.len()));
    reveal(state, &hits[next]);
}

/// Select the frame the hit is in and put the caret on the text.
fn reveal(state: &mut TesseraApp, hit: &find::Hit) {
    state.edit_master(None);
    let chain = state.active().document().thread_of(hit.frame);
    let frame = state.resolve_active().items.iter().find(|item| {
        chain.contains(&item.frame) && matches!(&item.kind,
            tessera_layout::ResolvedKind::Text { shaped, .. } if shaped.lines.iter().any(|l| l.range.contains(&hit.range.start)))
    }).map_or(hit.frame, |item| item.frame);
    state.active_mut().selection.set(frame);
    let doc = state.active().document();
    let spread = doc.page_of_frame(frame).and_then(|page| {
        doc.spread_ids()
            .position(|spread| doc.pages_of(spread).contains(&page))
    });
    if let Some(spread) = spread {
        state.active_mut().current_spread = spread;
    }
    state.reveal = Some(frame);
    crate::view::viewport::start_editing_cell(state, frame, hit.cell);
    if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
        buffer.select(hit.range.clone());
    }
}

/// Change the hit we are standing on, then go to the next.
fn change_one(state: &mut TesseraApp) {
    sync_context(state);
    let hits = find::search(state.active().document(), &state.find.query);
    let Some(at) = state.find.at.filter(|at| *at < hits.len()) else {
        // Nothing is standing on a hit yet, so the first press finds rather
        // than changes. Pressing Change before Find should not quietly alter
        // whichever occurrence happened to be first.
        go_to_next(state);
        return;
    };

    // The edit closes the editing session, because the buffer holds its own
    // copy of the story and would write a stale one back over the change.
    let edits = find::edits_for(&hits[at..=at], &state.find.replacement);
    crate::apply(state, crate::Command::ReplaceMatches { edits });

    // Resume beyond the inserted text, whose length may differ from the hit.
    let resume = hits[at].range.start + state.find.replacement.len();
    let remaining = find::search(state.active().document(), &state.find.query);
    sync_context(state);
    if remaining.is_empty() {
        state.find.note = Some("Changed 1 occurrence. No matches remain.".into());
    } else {
        // Skip matches inside the replacement itself, even when it contains
        // the search term (cat → catfish). Continue with the next original hit.
        let next = remaining
            .iter()
            .enumerate()
            .skip(at)
            .find(|(_, hit)| hit.story != hits[at].story || hit.range.start >= resume)
            .map_or(0, |(index, _)| index);
        select_hit(state, &remaining, next);
        state.find.note = Some(format!(
            "Changed 1 occurrence. {} of {}",
            next + 1,
            remaining.len()
        ));
    }
}

/// Change every hit in the document, as one undo entry.
fn change_every(state: &mut TesseraApp) {
    sync_context(state);
    let hits = find::search(state.active().document(), &state.find.query);
    if hits.is_empty() {
        state.find.note = Some("Not found.".into());
        return;
    }
    let count = hits.len();
    let edits = find::edits_for(&hits, &state.find.replacement);
    crate::apply(state, crate::Command::ReplaceMatches { edits });
    sync_context(state);
    state.find.at = None;
    state.find.note = Some(match count {
        1 => "Changed 1 occurrence.".into(),
        n => format!("Changed {n} occurrences."),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply};
    use tessera_geometry::DocRect;

    #[test]
    fn enter_and_shift_enter_keep_search_focus_and_navigate() {
        let mut state = a_document_saying("cat cat cat");
        state.find.query.needle = "cat".into();
        state.find.open();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = crate::headless_frame::frame(&ctx, Default::default(), |ui| {
                show(ui.ctx(), &mut state)
            });
        }
        for (shift, expected) in [(false, 0), (false, 1), (true, 0), (true, 2)] {
            let modifiers = egui::Modifiers {
                shift,
                ..Default::default()
            };
            let input = egui::RawInput {
                events: vec![
                    egui::Event::ModifiersChanged(modifiers),
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                ],
                ..Default::default()
            };
            let _ = crate::headless_frame::frame(&ctx, input, |ui| show(ui.ctx(), &mut state));
            assert_eq!(state.find.at, Some(expected));
        }
    }

    fn a_document_saying(text: &str) -> TesseraApp {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 200.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: text.into(),
            },
        );
        state
    }

    fn text_of(state: &TesseraApp) -> String {
        let doc = state.active().document();
        let story = doc.stories.keys().next().expect("a story");
        doc.story(story).expect("story").text.clone()
    }

    #[test]
    fn changing_every_occurrence_is_one_undo_entry() {
        // The point of doing it as one command. Four hundred separate changes
        // would need four hundred presses of Ctrl+Z to take back one action.
        let mut state = a_document_saying("a cat, a cat, and one more cat");
        state.find.query.needle = "cat".into();
        state.find.replacement = "dog".into();

        change_every(&mut state);
        assert_eq!(text_of(&state), "a dog, a dog, and one more dog");

        apply(&mut state, Command::Undo);
        assert_eq!(
            text_of(&state),
            "a cat, a cat, and one more cat",
            "one undo must take back the whole change"
        );
    }

    #[test]
    fn a_longer_replacement_does_not_shift_the_hits_after_it() {
        // The bug the back-to-front ordering exists to prevent: replacing left
        // to right moves every later offset by the difference in length, so
        // the second change lands inside the wrong word.
        let mut state = a_document_saying("ax bx cx");
        state.find.query.needle = "x".into();
        state.find.replacement = "yyyy".into();

        change_every(&mut state);
        assert_eq!(text_of(&state), "ayyyy byyyy cyyyy");
    }

    #[test]
    fn an_empty_replacement_deletes() {
        let mut state = a_document_saying("keep [cut] keep");
        state.find.query.needle = "[cut] ".into();
        state.find.replacement = String::new();

        change_every(&mut state);
        assert_eq!(text_of(&state), "keep keep");
    }

    #[test]
    fn find_next_walks_the_document_and_wraps() {
        let mut state = a_document_saying("one two one two one");
        state.find.query.needle = "one".into();

        for expected in ["1 of 3", "2 of 3", "3 of 3", "1 of 3"] {
            go_to_next(&mut state);
            assert_eq!(state.find.note.as_deref(), Some(expected));
        }
    }

    #[test]
    fn a_needle_that_is_not_there_says_so_and_changes_nothing() {
        let mut state = a_document_saying("nothing to see");
        state.find.query.needle = "absent".into();

        go_to_next(&mut state);
        assert_eq!(state.find.note.as_deref(), Some("Not found."));
        change_every(&mut state);
        assert_eq!(text_of(&state), "nothing to see");
    }

    #[test]
    fn changing_one_leaves_the_others_alone_and_moves_on() {
        let mut state = a_document_saying("cat cat cat");
        state.find.query.needle = "cat".into();
        state.find.replacement = "dog".into();

        go_to_next(&mut state); // 1 of 3
        change_one(&mut state);
        assert_eq!(text_of(&state), "dog cat cat");

        change_one(&mut state);
        assert_eq!(
            text_of(&state),
            "dog dog cat",
            "the second change must not skip an occurrence or repeat one"
        );
    }

    #[test]
    fn finding_a_table_hit_starts_editing_the_owning_cell() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTable {
                bounds: DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 100.0,
                },
                rows: 1,
                columns: 2,
            },
        );
        let frame = state.active().selection.single().unwrap();
        let story = crate::view::viewport::editing_story(&state, frame, Some((0, 1))).unwrap();
        apply(
            &mut state,
            Command::ReplaceMatches {
                edits: vec![(story, 0..0, "needle".into())],
            },
        );
        state.find.query.needle = "needle".into();
        go_to_next(&mut state);
        assert_eq!(state.active().editing_cell, Some((0, 1)));
        assert_eq!(
            state.active().editing.as_ref().unwrap().1.selected_text(),
            Some("needle")
        );
    }

    #[test]
    fn change_before_find_finds_first() {
        // Pressing Change with nothing standing on a hit must not quietly
        // alter whichever occurrence happened to be first.
        let mut state = a_document_saying("cat cat");
        state.find.query.needle = "cat".into();
        state.find.replacement = "dog".into();

        change_one(&mut state);
        assert_eq!(text_of(&state), "cat cat", "nothing may change yet");
        assert_eq!(state.find.note.as_deref(), Some("1 of 2"));
    }

    #[test]
    fn changing_the_query_requires_a_new_confirmed_hit() {
        let mut state = a_document_saying("cat dog dog");
        state.find.query.needle = "cat".into();
        go_to_next(&mut state);
        state.find.query.needle = "dog".into();
        state.find.replacement = "fox".into();
        change_one(&mut state);
        assert_eq!(text_of(&state), "cat dog dog");
        assert_eq!(state.find.at, Some(0));
    }

    #[test]
    fn switching_documents_requires_a_new_confirmed_hit() {
        let mut state = a_document_saying("cat cat");
        state.find.query.needle = "cat".into();
        go_to_next(&mut state);
        let other = a_document_saying("cat unrelated");
        state.add_document(other.active().document().clone(), None);
        state.find.replacement = "fox".into();
        change_one(&mut state);
        assert_eq!(text_of(&state), "cat unrelated");
    }

    #[test]
    fn editing_the_document_invalidates_a_confirmed_hit() {
        let mut state = a_document_saying("cat cat");
        state.find.query.needle = "cat".into();
        go_to_next(&mut state);
        let id = state.active().selection.single().unwrap();
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "new cat cat".into(),
            },
        );
        state.find.replacement = "fox".into();
        change_one(&mut state);
        assert_eq!(text_of(&state), "new cat cat");
    }

    #[test]
    fn replacement_containing_the_query_advances_past_inserted_text() {
        let mut state = a_document_saying("cat cat cat");
        state.find.query.needle = "cat".into();
        state.find.replacement = "catfish".into();
        go_to_next(&mut state);
        change_one(&mut state);
        change_one(&mut state);
        assert_eq!(text_of(&state), "catfish catfish cat");
    }

    #[test]
    fn previous_walks_backwards_and_wraps() {
        let mut state = a_document_saying("cat cat cat");
        state.find.query.needle = "cat".into();
        for expected in [2, 1, 0, 2] {
            go_to_previous(&mut state);
            assert_eq!(state.find.at, Some(expected));
        }
    }

    #[test]
    fn finding_a_hit_on_another_spread_reveals_it() {
        let mut state = a_document_saying("needle");
        let frame = state.active().selection.single().unwrap();
        apply(&mut state, Command::AddPage);
        let page = state.active().document().page_ids().last().unwrap();
        let bounds = state.active().document().pages[page].bounds;
        apply(&mut state, Command::SetBounds { id: frame, bounds });
        state.active_mut().current_spread = 0;
        state.find.query.needle = "needle".into();
        go_to_next(&mut state);
        assert_eq!(state.active().current_spread, 1);
        assert_eq!(state.reveal, Some(frame));
    }

    #[test]
    fn replacing_the_final_hit_reports_success() {
        let mut state = a_document_saying("cat");
        state.find.query.needle = "cat".into();
        state.find.replacement = "dog".into();
        go_to_next(&mut state);
        change_one(&mut state);
        assert_eq!(
            state.find.note.as_deref(),
            Some("Changed 1 occurrence. No matches remain.")
        );
        assert!(state.find.at.is_none());
    }
}
