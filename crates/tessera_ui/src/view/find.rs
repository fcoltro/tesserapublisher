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
    /// An index rather than a range, because the document can change under it:
    /// every button re-runs the search, so the position has to be expressed in
    /// terms the new results can still answer.
    pub at: Option<usize>,
    /// What the last action did, in words. `None` before anything is asked.
    pub note: Option<String>,
    /// Set when the window opens, so typing can start immediately.
    focus: bool,
}

impl FindWindow {
    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
        self.note = None;
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
        .default_width(360.0)
        .show(ctx, |ui| body(ui, state));
    if !open {
        state.find.open = false;
    }
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    ui.spacing_mut().item_spacing.y = Theme::SPACE_2;

    let needle = ui.horizontal(|ui| {
        ui.label("Find");
        ui.add(
            egui::TextEdit::singleline(&mut state.find.query.needle)
                .desired_width(f32::INFINITY)
                .hint_text("text to find"),
        )
    });
    let needle = needle.inner;
    if std::mem::take(&mut state.find.focus) {
        needle.request_focus();
    }

    ui.horizontal(|ui| {
        ui.label("Change to");
        ui.add(
            egui::TextEdit::singleline(&mut state.find.replacement)
                .desired_width(f32::INFINITY)
                .hint_text("leave empty to delete"),
        );
    });

    ui.horizontal(|ui| {
        ui.checkbox(&mut state.find.query.match_case, "Match case");
        ui.checkbox(&mut state.find.query.whole_word, "Whole word");
    });

    // Return in the find box is Find Next, which is what every search box in
    // every application does and what a person will try first.
    let entered = needle.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

    ui.separator();

    let runnable = state.find.query.is_runnable();
    let mut find_next = entered;
    let mut change = false;
    let mut change_all = false;

    ui.horizontal(|ui| {
        find_next |= ui
            .add_enabled(runnable, super::primary_button("Find next"))
            .clicked();
        change |= ui
            .add_enabled(runnable, egui::Button::new("Change"))
            .clicked();
        change_all |= ui
            .add_enabled(runnable, egui::Button::new("Change all"))
            .clicked();
    });

    if find_next {
        go_to_next(state);
    }
    if change {
        change_one(state);
    }
    if change_all {
        change_every(state);
    }

    if let Some(note) = &state.find.note {
        ui.colored_label(Theme::text_muted(), note);
    }
}

/// Move to the hit after the one we are on, wrapping at the end.
fn go_to_next(state: &mut TesseraApp) {
    let hits = find::search(state.active().document(), &state.find.query);
    if hits.is_empty() {
        state.find.at = None;
        state.find.note = Some("Not found.".into());
        return;
    }
    // Wrapping is what a person expects and InDesign asks about; asking is a
    // dialog in front of a dialog, and stopping dead at the last hit means a
    // search started halfway down the document never sees the top of it.
    let next = match state.find.at {
        Some(at) if at + 1 < hits.len() => at + 1,
        Some(_) => 0,
        None => 0,
    };
    state.find.at = Some(next);
    state.find.note = Some(format!("{} of {}", next + 1, hits.len()));
    reveal(state, &hits[next]);
}

/// Select the frame the hit is in and put the caret on the text.
fn reveal(state: &mut TesseraApp, hit: &find::Hit) {
    state.active_mut().selection.set(hit.frame);
    crate::view::viewport::start_editing(state, hit.frame);
    if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
        buffer.select(hit.range.clone());
    }
}

/// Change the hit we are standing on, then go to the next.
fn change_one(state: &mut TesseraApp) {
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
    state.active_mut().editing = None;
    let edits = find::edits_for(&hits[at..=at], &state.find.replacement);
    crate::apply(state, crate::Command::ReplaceMatches { edits });

    // Standing on the hit before the one we changed, so the search that
    // follows lands on the next occurrence rather than skipping it.
    state.find.at = at.checked_sub(1);
    state.find.note = Some("Changed.".into());
    go_to_next(state);
}

/// Change every hit in the document, as one undo entry.
fn change_every(state: &mut TesseraApp) {
    let hits = find::search(state.active().document(), &state.find.query);
    if hits.is_empty() {
        state.find.note = Some("Not found.".into());
        return;
    }
    let count = hits.len();
    state.active_mut().editing = None;
    let edits = find::edits_for(&hits, &state.find.replacement);
    crate::apply(state, crate::Command::ReplaceMatches { edits });
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
}
