//! The bar that says which documents are open, and which one you are in.
//!
//! The multi-document structure has been there since milestone 1.5 — the
//! application has always held a map of them — and until now there was no way to
//! see that a second one existed, let alone reach it. Opening two files meant
//! the first one silently became unreachable.
//!
//! ## Only when there is more than one
//!
//! A bar showing a single tab is a row of chrome that answers a question nobody
//! asked, on every document anybody ever opens. It appears when it means
//! something and takes no room the rest of the time.
//!
//! ## The dirty mark is a dot, not an asterisk in the name
//!
//! An asterisk changes the width of the tab as you type into it, which makes the
//! whole bar shift the first time a document becomes unsaved. A dot in a fixed
//! place does not move anything.

use egui::Ui;

use crate::app::{DocumentKey, TesseraApp};
use crate::theme::Theme;

/// How much of a long file name a tab shows.
///
/// Long enough to tell two versions of one job apart, short enough that six
/// documents still fit. A tab that shows the whole of
/// "Annual-Report-2026-final-v4-APPROVED.tessera" pushes the other five off.
const MOST_CHARACTERS: usize = 22;

/// The bar, when there is more than one document open.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    if state.documents.len() < 2 {
        return;
    }

    // Read out first: acting on a click needs the application mutably, and the
    // list is borrowed from it.
    let tabs: Vec<(DocumentKey, String, bool)> = state
        .documents
        .iter()
        .map(|(key, doc)| (key, name_of(doc), doc.dirty))
        .collect();

    let mut choose = None;
    let mut close = None;

    egui::ScrollArea::horizontal()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (key, name, dirty) in &tabs {
                    let active = *key == state.active;
                    let response = ui.selectable_label(active, shorten(name));

                    let response = if *dirty {
                        response.on_hover_text(format!("{name} — unsaved changes"))
                    } else {
                        response.on_hover_text(name)
                    };
                    if response.clicked() {
                        choose = Some(*key);
                    }

                    if *dirty {
                        // A dot beside the name rather than an asterisk in it: an
                        // asterisk changes the tab's width the moment somebody
                        // types, and the whole bar jumps.
                        ui.colored_label(Theme::accent(), "\u{2022}");
                    }

                    // Only on the tab you are in. A row of close buttons is a
                    // row of ways to lose a document by clicking a pixel to the
                    // right of the one you meant.
                    if active && tabs.len() > 1 && ui.small_button("\u{2715}").clicked() {
                        close = Some(*key);
                    }

                    ui.separator();
                }
            });
        });

    if let Some(key) = choose {
        state.active = key;
    }
    if let Some(key) = close {
        request_close(state, key);
    }
}

/// What a document is called in the bar.
fn name_of(doc: &crate::open_document::OpenDocument) -> String {
    doc.current_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string())
}

/// A name that fits, with an ellipsis where it was cut.
///
/// Cut from the **middle**, not the end: two files called
/// "Report-final-v3" and "Report-final-v4" differ in their last character, and
/// trimming the end would make them identical on screen.
fn shorten(name: &str) -> String {
    let characters: Vec<char> = name.chars().collect();
    if characters.len() <= MOST_CHARACTERS {
        return name.to_string();
    }
    let keep = MOST_CHARACTERS - 1;
    let head: String = characters[..keep / 2].iter().collect();
    let tail: String = characters[characters.len() - (keep - keep / 2)..]
        .iter()
        .collect();
    format!("{head}\u{2026}{tail}")
}

/// Close a document, refusing to throw away unsaved work silently.
///
/// **A tab with a dot on it does not close on one click.** Losing an afternoon
/// to a mis-aimed pointer is the failure this guards, and asking is cheap.
fn request_close(state: &mut TesseraApp, key: DocumentKey) {
    if state.documents.len() < 2 {
        // The last document is not closed, it is emptied — and there is nothing
        // here that does that yet, so refusing is honest.
        return;
    }

    if state.documents.get(key).is_some_and(|d| d.dirty) {
        state.closing = Some(key);
        return;
    }
    close_now(state, key);
}

/// Close it, and move to a neighbour.
pub fn close_now(state: &mut TesseraApp, key: DocumentKey) {
    // The next one along, or the previous if this was last. Jumping to the
    // first would lose your place in a way that has to be undone by hand.
    let order: Vec<DocumentKey> = state.documents.keys().collect();
    let at = order.iter().position(|k| *k == key);
    state.documents.remove(key);

    if state.active == key {
        let next = at
            .and_then(|at| {
                order
                    .get(at + 1)
                    .or_else(|| at.checked_sub(1).and_then(|p| order.get(p)))
            })
            .copied()
            .filter(|k| state.documents.contains_key(*k))
            .or_else(|| state.documents.keys().next());
        if let Some(next) = next {
            state.active = next;
        }
    }
}

/// The "you have unsaved changes" question, when one is pending.
pub fn confirm_close(ctx: &egui::Context, state: &mut TesseraApp) {
    let Some(key) = state.closing else {
        return;
    };
    let name = state
        .documents
        .get(key)
        .map(name_of)
        .unwrap_or_else(|| "This document".to_string());

    let mut decided = None;
    egui::Window::new("Unsaved changes")
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(format!("{name} has changes that have not been saved."));
            ui.add_space(Theme::SPACE_2);
            ui.horizontal(|ui| {
                // Save first, because it is what somebody who mis-clicked
                // wants, and the leftmost button is the one a hand goes to.
                if ui.button("Save and close").clicked() {
                    decided = Some(Decision::Save);
                }
                if ui.button("Cancel").clicked() {
                    decided = Some(Decision::Keep);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Furthest from the others, and named for what it does
                    // rather than "Don't save" — "Discard" is what actually
                    // happens to the afternoon.
                    if ui.button("Discard changes").clicked() {
                        decided = Some(Decision::Discard);
                    }
                });
            });
        });

    match decided {
        Some(Decision::Save) => {
            state.closing = None;
            let was = state.active;
            state.active = key;
            crate::file_ops::save(state);
            // Only if the save actually happened: a cancelled save dialog must
            // not close the document it was saving.
            if !state.documents.get(key).is_some_and(|d| d.dirty) {
                close_now(state, key);
            } else {
                state.active = was;
            }
        }
        Some(Decision::Discard) => {
            state.closing = None;
            close_now(state, key);
        }
        Some(Decision::Keep) => state.closing = None,
        None => {}
    }
}

enum Decision {
    Save,
    Discard,
    Keep,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_name_is_cut_in_the_middle() {
        // Two files called "Report-final-v3" and "Report-final-v4" differ in
        // their last character. Trimming the end would make them identical on
        // screen, which is the one thing a tab must not do.
        let a = shorten("Annual-Report-2026-final-approved-v3");
        let b = shorten("Annual-Report-2026-final-approved-v4");
        assert_ne!(a, b, "two documents became the same tab");
        assert!(a.chars().count() <= MOST_CHARACTERS);
    }

    #[test]
    fn a_short_name_is_left_alone() {
        assert_eq!(shorten("Poster"), "Poster");
    }

    /// Two documents open, the first with something in it.
    ///
    /// The something matters: an untouched blank is *replaced* rather than
    /// added to, which is deliberate and is tested on its own below.
    fn two() -> (TesseraApp, DocumentKey, DocumentKey) {
        let mut state = TesseraApp::headless();
        let first = state.active;
        crate::command::apply(
            &mut state,
            crate::command::Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        crate::file_ops::new_document(&mut state);
        let second = state.active;
        (state, first, second)
    }

    #[test]
    fn opening_a_second_document_keeps_the_first() {
        // The map has been here since milestone 1.5 and nothing ever put a
        // second thing in it: opening a file replaced the one you had.
        let (state, first, second) = two();
        assert_ne!(first, second);
        assert_eq!(state.documents.len(), 2);
    }

    #[test]
    fn the_blank_document_tessera_opens_with_is_reused_rather_than_left_beside() {
        // It is a placeholder, not work. Leaving it as a stray "Untitled" tab
        // is clutter somebody tidies on every launch.
        let mut state = TesseraApp::headless();
        crate::file_ops::new_document(&mut state);
        assert_eq!(state.documents.len(), 1, "a blank was left behind");
    }

    #[test]
    fn a_blank_document_somebody_has_worked_in_is_not_a_placeholder() {
        // However empty it looks. Undo history is work.
        let (state, _, _) = two();
        assert_eq!(state.documents.len(), 2);
    }

    #[test]
    fn closing_a_clean_document_moves_to_its_neighbour() {
        let (mut state, first, second) = two();

        close_now(&mut state, second);
        assert_eq!(state.active, first, "the neighbour was not taken");
        assert_eq!(state.documents.len(), 1);
    }

    #[test]
    fn closing_a_dirty_document_asks_first() {
        // Losing an afternoon to a mis-aimed pointer is the failure this guards.
        let (mut state, _, second) = two();
        crate::command::apply(
            &mut state,
            crate::command::Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        assert!(state.active().dirty);

        request_close(&mut state, second);
        assert_eq!(state.closing, Some(second), "it closed without asking");
        assert_eq!(state.documents.len(), 2, "and it closed");
    }

    #[test]
    fn the_last_document_is_not_closed() {
        // Closing it would leave an application with no document, which is a
        // state nothing else here can handle.
        let mut state = TesseraApp::headless();
        let only = state.active;
        request_close(&mut state, only);
        assert_eq!(state.documents.len(), 1);
        assert!(state.closing.is_none(), "it asked about the last document");
    }
}
