//! The command palette.
//!
//! D3. `command.rs` already routes every mutation through one enum and
//! `actions.rs` names every one of them, so this is a filter over a list that
//! exists rather than a new architecture.
//!
//! It also teaches shortcuts as a side effect of being used, which is why
//! each row shows one — and it discharges milestone 7's obligation that every
//! common command be reachable, whether or not it has a chord.

use egui::{Key, Ui};

use crate::actions;
use crate::app::TesseraApp;
use crate::theme::Theme;

/// The palette's own state. View state; nothing here is document data.
#[derive(Debug, Default)]
pub struct Palette {
    pub open: bool,
    pub query: String,
    /// Which row the arrow keys have moved to.
    pub highlighted: usize,
}

impl Palette {
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.highlighted = 0;
    }
}

/// Move the highlight within a list of `len` rows, wrapping at both ends.
///
/// Wrapping because a palette is a ring, not a page: pressing up on the first
/// row should reach the last rather than doing nothing.
pub fn moved(highlighted: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let len = len as i32;
    let at = highlighted.min(len as usize - 1) as i32;
    (((at + delta) % len) + len) as usize % len as usize
}

/// Draw the palette, and run whatever it is asked for.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    if state.new_document.open
        || state.quit.pending
        || state.closing.is_some()
        || state.export.open
        || state.step.open
    {
        return;
    }
    let toggle = ui
        .ctx()
        .input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::K));
    if toggle {
        let open = !state.palette.open;
        state.palette.close();
        state.palette.open = open;
    }
    if !state.palette.open {
        return;
    }

    let ctx = ui.ctx();
    let (up, down, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
        )
    });
    let mut chosen = None;
    let id = egui::Id::new("command-palette");
    let response = egui::Modal::new(id)
        .area(egui::Modal::default_area(id).anchor(
            egui::Align2::CENTER_TOP,
            egui::vec2(0.0, (ctx.content_rect().height() * 0.15).min(120.0)),
        ))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 80.0).clamp(240.0, 520.0));
            ui.horizontal(|ui| {
                ui.heading("Command search");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak("Esc to close");
                });
            });
            ui.add_space(Theme::space_2());
            let field = ui.add(
                egui::TextEdit::singleline(&mut state.palette.query)
                    .hint_text("Search tools and commands…")
                    .desired_width(f32::INFINITY),
            );
            field.request_focus();
            // Filter after the field reads this frame's input.
            let matches: Vec<_> = actions::filtered(&state.palette.query)
                .into_iter()
                .filter(|a| actions::enabled(state, a.run))
                .collect();
            if field.changed() {
                state.palette.highlighted = 0;
            }
            state.palette.highlighted = state
                .palette
                .highlighted
                .min(matches.len().saturating_sub(1));
            if up {
                state.palette.highlighted = moved(state.palette.highlighted, matches.len(), -1);
            }
            if down {
                state.palette.highlighted = moved(state.palette.highlighted, matches.len(), 1);
            }
            if enter {
                chosen = matches.get(state.palette.highlighted).map(|a| a.run);
            }
            ui.add_space(Theme::space_2());
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 240.0).clamp(100.0, 340.0))
                .show(ui, |ui| {
                    if matches.is_empty() {
                        ui.add_space(Theme::space_3());
                        ui.label("No available commands match your search.");
                        ui.weak("Try a tool name, such as Rectangle, or clear the search.");
                    }
                    for (i, action) in matches.iter().enumerate() {
                        let selected = i == state.palette.highlighted;
                        let shortcut = state
                            .prefs
                            .shortcuts
                            .chord(action)
                            .map(|c| c.label())
                            .unwrap_or_default();
                        let row = ui.add_sized(
                            [ui.available_width(), Theme::row() + Theme::space_1()],
                            egui::Button::new(action.name)
                                .selected(selected)
                                .shortcut_text(shortcut),
                        );
                        if selected && (up || down || field.changed()) {
                            row.scroll_to_me(Some(egui::Align::Center));
                        }
                        if row.clicked() {
                            chosen = Some(action.run);
                        }
                    }
                });
            ui.separator();
            ui.weak(format!(
                "{} commands  ·  Up / Down to navigate  ·  Enter to run",
                matches.len()
            ));
        });
    if response.should_close() {
        state.palette.close();
    } else if let Some(run) = chosen {
        state.palette.close();
        actions::run(state, run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrowing_the_query_and_enter_use_the_current_results() {
        let mut state = TesseraApp::headless();
        state.palette.open = true;
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ui| show(ui, &mut state));
        state.palette.highlighted = 99;
        let input = egui::RawInput {
            events: vec![
                egui::Event::Text("Rectangle tool".into()),
                egui::Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| show(ui, &mut state));
        assert_eq!(state.active_tool, crate::tools::Tool::Rectangle);
        assert!(!state.palette.open);
    }

    #[test]
    fn the_highlight_wraps_at_both_ends() {
        // A palette is a ring, not a page: up from the first row reaches the
        // last rather than doing nothing.
        assert_eq!(moved(0, 5, -1), 4);
        assert_eq!(moved(4, 5, 1), 0);
    }

    #[test]
    fn the_highlight_moves_one_row_at_a_time() {
        assert_eq!(moved(2, 5, 1), 3);
        assert_eq!(moved(2, 5, -1), 1);
    }

    #[test]
    fn an_empty_list_has_nowhere_to_move_to() {
        assert_eq!(moved(3, 0, 1), 0);
    }

    #[test]
    fn a_highlight_past_the_end_is_brought_back_in() {
        // The query narrows the list under the highlight, so this happens on
        // nearly every keystroke.
        assert_eq!(moved(99, 3, 1), 0);
    }

    #[test]
    fn closing_forgets_the_query() {
        // Otherwise reopening shows the last search rather than everything.
        let mut palette = Palette {
            open: true,
            query: "align".to_string(),
            highlighted: 4,
        };
        palette.close();
        assert!(!palette.open);
        assert!(palette.query.is_empty());
        assert_eq!(palette.highlighted, 0);
    }
}
