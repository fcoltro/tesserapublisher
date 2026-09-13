//! Closing the native window must protect every open document.

use crate::app::TesseraApp;
use crate::theme::Theme;

#[derive(Default)]
pub struct Quit {
    pub pending: bool,
    pub confirmed: bool,
}

/// Returns whether the operating system may close the window now.
pub fn request(state: &mut TesseraApp) -> bool {
    if state.quit.confirmed || !state.documents.values().any(|doc| doc.dirty) {
        return true;
    }
    state.quit.pending = true;
    state.palette.close();
    false
}

/// A cancelled or failed save leaves the application open at the original tab.
fn save_all(state: &mut TesseraApp, mut save: impl FnMut(&mut TesseraApp)) -> bool {
    let active = state.active;
    let dirty: Vec<_> = state
        .documents
        .iter()
        .filter(|(_, d)| d.dirty)
        .map(|(k, _)| k)
        .collect();
    for key in dirty {
        state.active = key;
        save(state);
        if state.active().dirty {
            state.active = active;
            return false;
        }
    }
    state.active = active;
    true
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.quit.pending {
        return;
    }
    let mut save = false;
    let mut discard = false;
    let mut cancel = false;
    let response = egui::Modal::new(egui::Id::new("quit-confirmation"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width(420.0_f32.min(ctx.content_rect().width() - 64.0).max(240.0));
            ui.heading("Save changes before quitting?");
            ui.label("These documents have unsaved changes.");
            ui.add_space(Theme::space_2());
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for doc in state.documents.values().filter(|d| d.dirty) {
                        ui.label(format!("• {}", doc.title()));
                    }
                });
            ui.add_space(Theme::space_3());
            ui.separator();
            ui.horizontal(|ui| {
                discard = ui.button("Discard and quit").clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    save = ui.add(super::primary_button("Save all and quit")).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        });
    if cancel || response.should_close() {
        state.quit.pending = false;
    } else if discard || (save && save_all(state, crate::file_ops::save)) {
        state.quit.confirmed = true;
        state.quit.pending = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_checks_dirty_background_tabs() {
        let mut state = TesseraApp::headless();
        state.active_mut().dirty = true;
        state.add_document(Default::default(), None);
        assert!(!state.active().dirty);
        assert!(!request(&mut state));
        assert!(state.quit.pending);
        state.quit.confirmed = true;
        assert!(request(&mut state));
    }

    #[test]
    fn cancelled_save_preserves_tabs_and_active_document() {
        let mut state = TesseraApp::headless();
        state.active_mut().dirty = true;
        state.add_document(Default::default(), None);
        state.active_mut().dirty = true;
        let active = state.active;
        assert!(!save_all(&mut state, |_| {}));
        assert_eq!(state.active, active);
        assert_eq!(state.documents.len(), 2);
        assert!(!state.quit.confirmed);
        assert!(save_all(&mut state, |s| s.active_mut().dirty = false));
        assert_eq!(state.active, active);
        assert!(request(&mut state));
    }
}
