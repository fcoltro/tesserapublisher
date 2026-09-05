//! Canvas text entry.
//!
//! egui's own `TextEdit` state is deliberately **not** used. The cursor lives
//! in [`EditBuffer`], which is persistent application state, because an
//! immediate-mode widget is reconstructed every frame and a cursor it owned
//! could not survive (decision D3).

use egui::{Event, ImeEvent, Key, Ui};
use tessera_text::edit::EditBuffer;

/// Feed this frame's input into the buffer. Returns whether the text changed.
pub fn handle_events(ui: &Ui, buffer: &mut EditBuffer) -> bool {
    let mut changed = false;

    let events = ui.input(|i| i.events.clone());
    for event in events {
        match event {
            Event::Text(text) => {
                buffer.insert(&text);
                changed = true;
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => match key {
                Key::Backspace => {
                    buffer.delete_backward();
                    changed = true;
                }
                Key::Delete => {
                    buffer.delete_forward();
                    changed = true;
                }
                Key::ArrowLeft => buffer.move_left(modifiers.shift),
                Key::ArrowRight => buffer.move_right(modifiers.shift),
                Key::A if modifiers.command => buffer.select_all(),
                Key::Enter => {
                    buffer.insert("\n");
                    changed = true;
                }
                _ => {}
            },
            // The system clipboard. egui turns the platform's paste into an
            // event rather than a key, which is why this is not a `Key::V`
            // arm — and why text arriving from another application works at
            // all.
            Event::Paste(text) => {
                buffer.insert(&text);
                changed = true;
            }
            Event::Copy => {
                if let Some(text) = buffer.selected_text() {
                    ui.ctx().copy_text(text.to_string());
                }
            }
            Event::Cut => {
                if let Some(text) = buffer.selected_text() {
                    ui.ctx().copy_text(text.to_string());
                    // `insert("")` would do nothing; deleting backward over a
                    // selection is how the buffer removes one.
                    buffer.delete_backward();
                    changed = true;
                }
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) => {
                // An empty preedit means the IME was dismissed.
                buffer.set_ime_preedit(Some(text));
            }
            Event::Ime(ImeEvent::Commit(text)) => {
                buffer.insert(&text);
                changed = true;
            }
            _ => {}
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_text::story::Story;

    /// Drives a real `egui::Context` headlessly by injecting raw input, so
    /// these are genuine tests of the event plumbing rather than a mock.
    fn run_with_events(events: Vec<Event>, buffer: &mut EditBuffer) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        // `run_ui` hands the closure the root Ui directly, matching how
        // eframe 0.35 drives an application.
        let _ = ctx.run_ui(input, |ui| {
            handle_events(ui, buffer);
        });
    }

    fn key(key: Key, modifiers: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn a_text_event_reaches_the_buffer() {
        let mut buffer = EditBuffer::new(Story::default());
        run_with_events(vec![Event::Text("Hi".to_string())], &mut buffer);
        assert_eq!(buffer.story().text, "Hi");
    }

    #[test]
    fn backspace_reaches_the_buffer() {
        let mut buffer = EditBuffer::new(Story::new("Hi"));
        buffer.set_cursor(2);
        run_with_events(
            vec![key(Key::Backspace, egui::Modifiers::NONE)],
            &mut buffer,
        );
        assert_eq!(buffer.story().text, "H");
    }

    #[test]
    fn enter_inserts_a_newline_rather_than_ending_the_edit() {
        let mut buffer = EditBuffer::new(Story::new("a"));
        buffer.set_cursor(1);
        run_with_events(vec![key(Key::Enter, egui::Modifiers::NONE)], &mut buffer);
        assert_eq!(buffer.story().text, "a\n");
    }

    #[test]
    fn shift_arrow_extends_a_selection() {
        let mut buffer = EditBuffer::new(Story::new("abc"));
        buffer.set_cursor(0);
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        run_with_events(
            vec![key(Key::ArrowRight, shift), key(Key::ArrowRight, shift)],
            &mut buffer,
        );
        assert_eq!(buffer.selection_range(), Some(0..2));
    }

    #[test]
    fn an_ime_preedit_reaches_the_buffer_without_committing() {
        let mut buffer = EditBuffer::new(Story::default());
        run_with_events(
            vec![Event::Ime(ImeEvent::Preedit {
                text: "に".to_string(),
                active_range_chars: None,
            })],
            &mut buffer,
        );
        assert_eq!(buffer.ime_preedit(), Some("に"));
        assert_eq!(buffer.story().text, "");
    }

    #[test]
    fn an_ime_commit_enters_the_text() {
        let mut buffer = EditBuffer::new(Story::default());
        run_with_events(
            vec![Event::Ime(ImeEvent::Commit("日本".to_string()))],
            &mut buffer,
        );
        assert_eq!(buffer.story().text, "日本");
    }

    #[test]
    fn a_frame_with_no_events_changes_nothing() {
        let mut buffer = EditBuffer::new(Story::new("stable"));
        run_with_events(Vec::new(), &mut buffer);
        assert_eq!(buffer.story().text, "stable");
    }

    // --- the system clipboard ----------------------------------------------

    #[test]
    fn text_pasted_from_another_application_arrives_in_the_buffer() {
        // Reported from real use: pasting into a text frame did nothing. egui
        // delivers the platform's paste as an event rather than as Ctrl+V, and
        // nothing was listening for it.
        let mut buffer = EditBuffer::new(Story::new("start "));
        buffer.set_cursor(6);

        run_with_events(vec![Event::Paste("pasted".to_string())], &mut buffer);

        assert_eq!(buffer.story().text, "start pasted");
        assert!(buffer.story().runs_are_sound());
    }

    #[test]
    fn a_paste_replaces_the_selection() {
        let mut buffer = EditBuffer::new(Story::new("keep this"));
        buffer.select(5..9);

        run_with_events(vec![Event::Paste("that".to_string())], &mut buffer);

        assert_eq!(buffer.story().text, "keep that");
        assert!(buffer.story().runs_are_sound());
    }

    #[test]
    fn a_multi_line_paste_keeps_its_paragraphs() {
        // Text from another application arrives with its newlines, and each
        // one is a paragraph the layout has to see.
        let mut buffer = EditBuffer::new(Story::new(""));

        run_with_events(
            vec![Event::Paste("one\ntwo\nthree".to_string())],
            &mut buffer,
        );

        assert_eq!(buffer.story().text, "one\ntwo\nthree");
        assert!(buffer.story().runs_are_sound());
    }

    #[test]
    fn pasting_takes_the_formatting_chosen_at_the_caret() {
        // Pasted text is text: it lands where the caret is and takes what the
        // caret was told to be, exactly as typed text does.
        use tessera_text::story::CharacterFormat;

        let mut buffer = EditBuffer::new(Story::new("ab"));
        buffer.set_cursor(2);
        buffer.set_pending(&CharacterFormat {
            weight: Some(700),
            ..CharacterFormat::default()
        });

        run_with_events(vec![Event::Paste("cd".to_string())], &mut buffer);

        assert_eq!(
            buffer.story().run_at(2).and_then(|r| r.local.weight),
            Some(700)
        );
    }

    #[test]
    fn cutting_removes_the_selection() {
        let mut buffer = EditBuffer::new(Story::new("keep this"));
        buffer.select(4..9);

        run_with_events(vec![Event::Cut], &mut buffer);

        assert_eq!(buffer.story().text, "keep");
        assert!(buffer.story().runs_are_sound());
    }

    #[test]
    fn cutting_nothing_removes_nothing() {
        // A caret is not a selection, and cutting with one must not eat the
        // character before it.
        let mut buffer = EditBuffer::new(Story::new("intact"));
        buffer.set_cursor(3);

        run_with_events(vec![Event::Cut], &mut buffer);

        assert_eq!(buffer.story().text, "intact");
    }

    #[test]
    fn copying_leaves_the_text_alone() {
        let mut buffer = EditBuffer::new(Story::new("keep this"));
        buffer.select(0..4);

        run_with_events(vec![Event::Copy], &mut buffer);

        assert_eq!(buffer.story().text, "keep this");
    }

    #[test]
    fn the_selection_is_what_gets_copied() {
        let mut buffer = EditBuffer::new(Story::new("keep this"));
        buffer.select(5..9);
        assert_eq!(buffer.selected_text(), Some("this"));

        buffer.set_cursor(0);
        assert_eq!(
            buffer.selected_text(),
            None,
            "a caret has nothing to put on the clipboard"
        );
    }
}
