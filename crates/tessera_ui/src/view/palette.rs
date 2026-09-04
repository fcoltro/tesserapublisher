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
    let window = ui.max_rect();
    // Ctrl+K opens it; Escape closes it without running anything.
    let toggle = ui
        .ctx()
        .input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::K));
    if toggle {
        state.palette.open = !state.palette.open;
        state.palette.query.clear();
        state.palette.highlighted = 0;
    }
    if !state.palette.open {
        return;
    }
    if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
        state.palette.close();
        return;
    }

    let matches = actions::filtered(&state.palette.query);
    let (up, down, enter) = ui.ctx().input(|i| {
        (
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Enter),
        )
    });
    if up {
        state.palette.highlighted = moved(state.palette.highlighted, matches.len(), -1);
    }
    if down {
        state.palette.highlighted = moved(state.palette.highlighted, matches.len(), 1);
    }

    let mut chosen = None;
    if enter {
        chosen = matches.get(state.palette.highlighted).map(|a| a.run);
    }

    egui::Area::new(egui::Id::new("command-palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(window.center().x - 220.0, window.min.y + 80.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(Theme::PANEL_BG)
                .show(ui, |ui| {
                    ui.set_width(440.0);
                    let field = ui.text_edit_singleline(&mut state.palette.query);
                    field.request_focus();
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            if matches.is_empty() {
                                ui.colored_label(Theme::TEXT_MUTED, "No matching command");
                            }
                            for (i, action) in matches.iter().enumerate() {
                                let selected = i == state.palette.highlighted;
                                let row = ui.horizontal(|ui| {
                                    let label = ui.selectable_label(selected, action.name);
                                    if let Some(shortcut) = action.shortcut {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.colored_label(Theme::TEXT_MUTED, shortcut);
                                            },
                                        );
                                    }
                                    label
                                });
                                if row.inner.clicked() {
                                    chosen = Some(action.run);
                                }
                            }
                        });
                });
        });

    if let Some(run) = chosen {
        state.palette.close();
        actions::run(state, run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
