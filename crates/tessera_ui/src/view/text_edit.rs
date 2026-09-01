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
}
