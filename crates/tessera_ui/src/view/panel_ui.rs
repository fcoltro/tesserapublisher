//! Shared, width-aware controls for the docked resource panels.
use egui::{Response, Ui};

use crate::{icons::Icon, theme::Theme};

/// A familiar symbol paired with its action, with one focus and hit target.
pub fn action(ui: &mut Ui, icon: Icon, label: &str) -> Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = if ui.is_enabled() {
        Theme::text_primary()
    } else {
        Theme::text_muted()
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, color);
    let width = (galley.size().x + 38.0).min(ui.available_width());
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, Theme::row()), egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        Theme::RADIUS,
        if response.hovered() {
            Theme::hover_bg()
        } else {
            Theme::panel_bg_alt()
        },
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            Theme::RADIUS,
            egui::Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }
    crate::icons::paint(
        &painter,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 16.0, rect.center().y),
            egui::Vec2::splat(18.0),
        ),
        icon,
        color,
    );
    painter.galley(
        egui::pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    crate::icons::named(response, label)
}

pub fn hint(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).small().color(Theme::text_muted())).wrap());
}

pub fn empty(ui: &mut Ui, title: &str, detail: &str) {
    egui::Frame::new()
        .fill(Theme::panel_bg_alt())
        .inner_margin(12)
        .corner_radius(Theme::RADIUS as u8)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(title).strong());
            hint(ui, detail);
        });
    ui.add_space(Theme::space_2());
}

pub fn entry(ui: &mut Ui, selected: bool, label: &str) -> Response {
    ui.add_sized(
        [ui.available_width(), Theme::row()],
        egui::Button::selectable(selected, label).truncate(),
    )
    .on_hover_text(label)
}

/// A row naming a thing, read from the left, with `reserved` points kept
/// clear on its right for whatever the caller paints there.
///
/// `entry` centres its label and truncates it against the whole width, which
/// is right for a style's name and wrong for a file's: names are scanned
/// down the left edge, and a long one would run under the marks beside it.
pub fn named_row(ui: &mut Ui, selected: bool, label: &str, reserved: f32) -> Response {
    let width = ui.available_width();
    let text_width = (width - reserved - 2.0 * Theme::space_2()).max(0.0);
    ui.allocate_ui_with_layout(
        egui::vec2(width, Theme::row()),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(width);
            // Truncated by hand, against the room the text really has,
            // because the button would truncate against its whole width.
            let font = egui::TextStyle::Body.resolve(ui.style());
            let shown = ui.fonts_mut(|f| {
                let mut job = egui::text::LayoutJob::simple_singleline(
                    label.to_owned(),
                    font,
                    ui.visuals().text_color(),
                );
                job.wrap.max_width = text_width;
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                f.layout_job(job)
            });
            // A growing atom after the text takes up the rest of the row,
            // which is what holds the text at the left: `add_sized` centres
            // what it places, and so does a button with nothing to grow.
            ui.add(
                egui::Button::selectable(selected, (shown, egui::Atom::grow()))
                    .min_size(egui::vec2(width, Theme::row())),
            )
        },
    )
    .inner
    .on_hover_text(label)
}

#[cfg(test)]
mod tests {
    use crate::{
        app::TesseraApp,
        command::{Command, apply},
        view::rail::Dock,
    };

    #[test]
    fn resource_panels_fit_narrow_docks_with_empty_and_populated_lists() {
        for width in [208.0, 288.0] {
            for populated in [false, true] {
                for dock in Dock::ALL
                    .into_iter()
                    .filter(|dock| *dock != Dock::Properties)
                {
                    let ctx = egui::Context::default();
                    crate::theme::apply(&ctx);
                    let mut state = TesseraApp::headless();
                    if populated {
                        apply(&mut state, Command::AddLayer);
                        apply(
                            &mut state,
                            Command::DefineParagraphStyle(tessera_text::story::ParagraphStyle {
                                name: "A long editorial style name that must fit in the panel"
                                    .into(),
                                based_on: None,
                                format: Default::default(),
                            }),
                        );
                        state.styles_window.paragraph =
                            state.active().document().paragraph_styles.keys().last();
                        apply(
                            &mut state,
                            Command::SetSwatch(tessera_document::nodes::Swatch {
                                name: "A long brand colour name that must fit in the panel".into(),
                                colour: tessera_color::Color::BLACK,
                                spot: false,
                            }),
                        );
                        state.swatches_window.chosen = state
                            .active()
                            .document()
                            .swatches
                            .first()
                            .map(|s| s.name.clone());
                        state.book.path = Some("a long publication name.book".into());
                        state
                            .book
                            .book
                            .documents
                            .push("a very long chapter document name.tsrdf".into());
                    }
                    for _ in 0..3 {
                        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                            ui.set_width(width);
                            let left = ui.cursor().left();
                            match dock {
                                Dock::Pages => super::super::pages::docked(ui, &mut state),
                                Dock::Layers => super::super::layers::docked(ui, &mut state),
                                Dock::Styles => super::super::styles::docked(ui, &mut state),
                                Dock::Swatches => super::super::swatches::docked(ui, &mut state),
                                Dock::Glyphs => super::super::glyphs::docked(ui, &mut state),
                                Dock::Book => super::super::book::docked(ui, &mut state),
                                Dock::Links => super::super::links::docked(ui, &mut state),
                                Dock::Preflight => {
                                    super::super::preflight_panel::docked(ui, &mut state)
                                }
                                Dock::Console => super::super::console::docked(ui, &mut state),
                                Dock::Properties => unreachable!(),
                            }
                            assert!(
                                ui.min_rect().right() <= left + width + 1.0,
                                "{dock:?} (populated={populated}) overflowed {width}: {:?}",
                                ui.min_rect()
                            );
                        });
                    }
                }
            }
        }
    }
}
