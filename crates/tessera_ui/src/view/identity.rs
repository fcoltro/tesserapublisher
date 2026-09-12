//! A compact theme button using the same outline icons as the rest of the UI.

use egui::Ui;

use crate::app::TesseraApp;
use crate::icons::{self, Icon};
use crate::prefs::ThemeChoice;
use crate::theme::Theme;

pub fn theme_switch(ui: &mut Ui, state: &mut TesseraApp) {
    let dark = state.prefs.theme == ThemeChoice::Dark;
    let (icon, target, label) = if dark {
        (Icon::Sun, ThemeChoice::Light, "Switch to light theme")
    } else {
        (Icon::Moon, ThemeChoice::Dark, "Switch to dark theme")
    };
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(28.0), egui::Sense::click());
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            Theme::RADIUS,
            egui::Stroke::new(1.0, Theme::accent()),
            egui::StrokeKind::Inside,
        );
    }
    icons::paint(ui.painter(), rect, icon, Theme::text_primary());
    if icons::named(response, label).clicked() {
        state.prefs.theme = target;
        crate::theme::follow(ui.ctx(), target);
        crate::prefs::remember(state);
    }
}
