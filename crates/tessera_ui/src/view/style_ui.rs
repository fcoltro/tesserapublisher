//! What the style window is drawn from: a sidebar, cards, rows that say
//! whether the style states them, segmented choices and switches.
//!
//! In a module of its own rather than beside the inspector's widgets,
//! because the window is laid out for room — seven hundred points, not a
//! 292-point rail — and its parts are sized for that. The colours are all
//! the theme's: nothing here picks a hue, so the window follows the light
//! and dark palettes and the contrast tests that hold them.
//!
//! ## The look
//!
//! One surface per job. The sidebar sits on the deepest ground, the pages
//! on the panel's, and each group of properties on a card raised a step
//! above it, so a page reads as a few named groups rather than one column of
//! thirty rows. The accent marks only two things: where you are (the page in
//! the sidebar) and what this style states (the dots, the chosen segment, a
//! switch that is on). Everything a style inherits is drawn in the muted
//! text colour, so a glance down a page separates the two.

use egui::{Color32, FontFamily, FontId, Rect, Response, Sense, Stroke, Ui, Vec2};

use crate::icons::Icon;
use crate::theme::Theme;

/// How far a card's corners round.
pub(crate) const CARD_RADIUS: u8 = 8;
/// A property row's height: a control's, with air above and below it.
pub(crate) const ROW: f32 = 30.0;
/// The column the property names sit in.
pub(crate) const NAME_COLUMN: f32 = 136.0;
/// The room at a row's start for the dot that says whether it is stated.
pub(crate) const DOT_COLUMN: f32 = 22.0;
/// How wide a number field is: "1440 pt" and a little air.
pub(crate) const NUMBER_WIDTH: f32 = 96.0;
/// The height of a segmented choice and a field.
const CONTROL: f32 = 24.0;

/// A card: raised a step above the page in the dark palette, and white
/// paper on the light one.
pub(crate) fn card_fill() -> Color32 {
    if Theme::is_light() {
        Theme::field_bg()
    } else {
        Theme::panel_bg_alt()
    }
}

/// The sidebar's ground: the deepest in the dark palette, a step under the
/// page in the light one.
pub(crate) fn sidebar_fill() -> Color32 {
    let palette = crate::theme::palette();
    if Theme::is_light() {
        palette.step(3)
    } else {
        palette.step(1)
    }
}

/// The heading face at `size`.
pub(crate) fn heading_font(size: f32) -> FontId {
    FontId::new(
        size,
        FontFamily::Name(crate::ui_fonts::HEADING_FAMILY.into()),
    )
}

/// A small capitalised caption: a card's title, a sidebar group's name.
pub(crate) fn overline(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text.to_uppercase())
                .font(heading_font(Theme::TYPE_SM - 1.0))
                .color(Theme::text_muted())
                .extra_letter_spacing(0.8),
        )
        .selectable(false),
    );
}

/// A group of rows on a raised surface, its title inside it.
pub(crate) fn card<R>(ui: &mut Ui, title: Option<&str>, add: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = egui::Frame::new()
        .fill(card_fill())
        .stroke(Stroke::new(1.0, Theme::rule()))
        .corner_radius(CARD_RADIUS)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(title) = title {
                overline(ui, title);
                ui.add_space(2.0);
            }
            add(ui)
        })
        .inner;
    ui.add_space(Theme::space_3());
    inner
}

/// A page's heading: its icon on a tinted tile, its name, and a line on
/// what it is for, with room at the right for the page's own action.
pub(crate) fn page_header(
    ui: &mut Ui,
    icon: Icon,
    title: &str,
    description: &str,
    trailing: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        let (tile, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
        ui.painter().rect_filled(tile, 8.0, Theme::accent_soft());
        crate::icons::paint(ui.painter(), tile, icon, Theme::text_primary());
        ui.add_space(2.0);
        ui.vertical(|ui| {
            ui.add_space(1.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .font(heading_font(16.0))
                        .color(Theme::text_primary()),
                )
                .selectable(false),
            );
            ui.add(
                egui::Label::new(egui::RichText::new(description).color(Theme::text_muted()))
                    .truncate()
                    .selectable(false),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), trailing);
    });
    ui.add_space(Theme::space_3());
}

/// One page in the sidebar: its icon, its name, and a badge counting what
/// the style states there.
///
/// The chosen page is marked twice — a tinted ground and a bar of accent at
/// its edge — because the ground alone, on the sidebar's deepest grey, was
/// a difference of one step and read as a hover.
pub(crate) fn nav_entry(
    ui: &mut Ui,
    icon: Icon,
    title: &str,
    stated: usize,
    selected: bool,
) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 6.0, Theme::accent_soft());
        painter.rect_filled(
            Rect::from_min_size(
                egui::pos2(rect.left(), rect.center().y - 8.0),
                Vec2::new(3.0, 16.0),
            ),
            1.5,
            Theme::accent(),
        );
    } else if response.hovered() {
        painter.rect_filled(rect, 6.0, Theme::hover_bg());
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }

    let glyph = Rect::from_min_size(
        egui::pos2(rect.left() + 8.0, rect.center().y - Theme::ICON_SIZE / 2.0),
        Vec2::splat(Theme::ICON_SIZE),
    );
    crate::icons::paint(
        painter,
        glyph,
        icon,
        if selected {
            Theme::text_primary()
        } else {
            Theme::text_muted()
        },
    );

    // The count, as a pill at the right. Measured first, so the name is
    // cut short of it rather than running under it.
    let mut right = rect.right() - 8.0;
    if stated > 0 {
        let text = stated.to_string();
        let galley = painter.layout_no_wrap(
            text,
            FontId::proportional(Theme::TYPE_SM),
            if selected {
                crate::theme::readable_on(Theme::accent())
            } else {
                Theme::text_primary()
            },
        );
        let width = (galley.size().x + 10.0).max(20.0);
        let pill = Rect::from_min_size(
            egui::pos2(right - width, rect.center().y - 9.0),
            Vec2::new(width, 18.0),
        );
        painter.rect_filled(
            pill,
            9.0,
            if selected {
                Theme::accent()
            } else {
                Theme::selected_bg()
            },
        );
        painter.galley(
            pill.center() - galley.size() / 2.0,
            galley,
            Theme::text_primary(),
        );
        right = pill.left() - 6.0;
    }
    let left = glyph.right() + 8.0;
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.to_owned(),
        FontId::proportional(Theme::TYPE_MD),
        Theme::text_primary(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(0.0));
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        Theme::text_primary(),
    );

    let name = match stated {
        0 => title.to_string(),
        1 => format!("{title}, 1 property stated"),
        n => format!("{title}, {n} properties stated"),
    };
    crate::icons::reads_as(
        response,
        name,
        egui::WidgetType::SelectableLabel,
        Some(selected),
    )
    .on_hover_text(title)
}

/// A row of a card: a fixed height, its parts left to right, lit when the
/// pointer is over it so the name and the control read as one line.
pub(crate) fn row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let ground = ui.painter().add(egui::Shape::Noop);
    let inner = ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), ROW),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(ROW);
            add(ui)
        },
    );
    let rect = inner.response.rect;
    if ui.rect_contains_pointer(rect) {
        ui.painter().set(
            ground,
            egui::epaint::RectShape::filled(
                rect.expand2(Vec2::new(6.0, 0.0)),
                6.0,
                Theme::hover_bg(),
            ),
        );
    }
    inner.inner
}

/// The dot at the start of a property's row: filled in the accent when this
/// style states the property, an empty ring when it inherits it. A click
/// turns one into the other.
///
/// It replaces a checkbox. A column of checkboxes read as a list of things
/// switched on and off, which is not what it was: an unticked size is not
/// "no size", it is somebody else's size. The ring says "not here", and the
/// value beside it, greyed, says whose.
pub(crate) fn state_dot(ui: &mut Ui, stated: bool, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(DOT_COLUMN, ROW), Sense::click());
    let painter = ui.painter();
    let centre = rect.center();
    if response.hovered() {
        painter.circle_filled(centre, 9.0, Theme::selected_bg());
    }
    if stated {
        painter.circle_filled(centre, 4.5, Theme::accent());
    } else {
        painter.circle_stroke(
            centre,
            4.0,
            Stroke::new(
                1.5,
                if response.hovered() {
                    Theme::accent()
                } else {
                    Theme::focus()
                },
            ),
        );
    }
    if response.has_focus() {
        painter.circle_stroke(centre, 9.0, Stroke::new(1.0, Theme::focus()));
    }
    crate::icons::reads_as(response, label, egui::WidgetType::Checkbox, Some(stated)).on_hover_text(
        if stated {
            "Stated by this style. Click to inherit it instead."
        } else {
            "Inherited. Click to state it in this style."
        },
    )
}

/// A property's name, in its column: the text colour when the style states
/// it, muted when it inherits it.
pub(crate) fn name_cell(ui: &mut Ui, label: &str, stated: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(NAME_COLUMN, ROW), Sense::hover());
    if label.is_empty() {
        return;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        FontId::proportional(Theme::TYPE_MD),
        if stated {
            Theme::text_primary()
        } else {
            Theme::text_muted()
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(NAME_COLUMN - 8.0);
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        Theme::text_primary(),
    );
}

/// Draw egui's own controls — a number, a drop-down, a text field — as a
/// value the style only inherits: muted text on no ground, a quieter edge.
/// Still live: editing one is how the value comes to be stated.
pub(crate) fn ghostly(ui: &mut Ui) {
    let visuals = ui.visuals_mut();
    visuals.override_text_color = Some(Theme::text_muted());
    visuals.text_edit_bg_color = Some(Color32::TRANSPARENT);
    let inactive = &mut visuals.widgets.inactive;
    inactive.bg_fill = Color32::TRANSPARENT;
    inactive.weak_bg_fill = Color32::TRANSPARENT;
    inactive.bg_stroke = Stroke::new(1.0, Theme::rule());
}

/// One choice in a segmented control: a word, or a picture with the word
/// kept for the tooltip and the screen reader.
#[derive(Clone, Copy)]
pub(crate) enum Segment<'a> {
    Text(&'a str),
    Icon(Icon, &'a str),
}

impl Segment<'_> {
    fn name(&self) -> &str {
        match self {
            Segment::Text(name) | Segment::Icon(_, name) => name,
        }
    }
}

/// A set of choices as one control: joined segments in a well, the chosen
/// one raised. Returns whether the choice changed.
///
/// `ghost` draws the choice an inherited value would make: marked, but in
/// grey rather than the accent, because this style is not the one making it.
pub(crate) fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut T,
    options: &[(Segment<'_>, T)],
    ghost: bool,
) -> bool {
    let font = FontId::proportional(Theme::TYPE_MD);
    let widths: Vec<f32> = options
        .iter()
        .map(|(segment, _)| match segment {
            Segment::Text(text) => {
                ui.painter()
                    .layout_no_wrap((*text).to_owned(), font.clone(), Theme::text_primary())
                    .size()
                    .x
                    + 20.0
            }
            Segment::Icon(..) => 30.0,
        })
        .collect();
    let inset = 2.0;
    let total = widths.iter().sum::<f32>() + 2.0 * inset;
    let (well, _) = ui.allocate_exact_size(Vec2::new(total, CONTROL), Sense::hover());
    ui.painter().rect(
        well,
        6.0,
        Theme::field_bg(),
        Stroke::new(
            1.0,
            if ghost {
                Theme::rule()
            } else {
                Theme::border()
            },
        ),
        egui::StrokeKind::Inside,
    );

    let mut changed = false;
    let mut x = well.left() + inset;
    let base = ui.id().with(("segmented", label));
    for (i, ((segment, candidate), width)) in options.iter().zip(&widths).enumerate() {
        let rect = Rect::from_min_size(
            egui::pos2(x, well.top() + inset),
            Vec2::new(*width, CONTROL - 2.0 * inset),
        );
        x += width;
        let response = ui.interact(rect, base.with(i), Sense::click());
        let chosen = *value == *candidate;
        let painter = ui.painter();
        if chosen {
            if ghost {
                painter.rect_filled(rect, 4.0, Theme::selected_bg());
            } else {
                painter.rect(
                    rect,
                    4.0,
                    Theme::accent_soft(),
                    Stroke::new(1.0, Theme::accent_edge()),
                    egui::StrokeKind::Inside,
                );
            }
        } else if response.hovered() {
            painter.rect_filled(rect, 4.0, Theme::hover_bg());
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0, Theme::focus()),
                egui::StrokeKind::Inside,
            );
        }
        let ink = if chosen && !ghost {
            Theme::text_primary()
        } else {
            Theme::text_muted()
        };
        match segment {
            Segment::Text(text) => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    *text,
                    font.clone(),
                    ink,
                );
            }
            Segment::Icon(icon, _) => crate::icons::paint(painter, rect, *icon, ink),
        }
        let response = crate::icons::reads_as(
            response,
            segment.name(),
            egui::WidgetType::RadioButton,
            Some(chosen),
        );
        let response = match segment {
            Segment::Icon(_, name) => response.on_hover_text(*name),
            Segment::Text(_) => response,
        };
        if response.clicked() && !chosen {
            *value = *candidate;
            changed = true;
        }
    }
    changed
}

/// An on-off switch: a track and a knob that slides.
///
/// For a flag a style states — italic, ligatures, hyphenate — where a
/// second checkbox beside the row's dot was two boxes on one line and no
/// telling which was which. `ghost` greys the track, as for a segment.
pub(crate) fn switch(ui: &mut Ui, on: &mut bool, label: &str, ghost: bool) -> bool {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(34.0, 18.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let slid = ui.ctx().animate_bool_responsive(response.id, *on);
    let painter = ui.painter();
    let track = if *on {
        if ghost {
            Theme::focus()
        } else {
            Theme::accent()
        }
    } else {
        Theme::selected_bg()
    };
    painter.rect(
        rect,
        9.0,
        track,
        Stroke::new(
            1.0,
            if *on {
                Color32::TRANSPARENT
            } else {
                Theme::border()
            },
        ),
        egui::StrokeKind::Inside,
    );
    let knob = egui::pos2(
        egui::lerp((rect.left() + 9.0)..=(rect.right() - 9.0), slid),
        rect.center().y,
    );
    painter.circle_filled(knob, 6.5, Color32::from_gray(250));
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            11.0,
            Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Outside,
        );
    }
    crate::icons::reads_as(
        response.clone(),
        label,
        egui::WidgetType::Checkbox,
        Some(*on),
    );
    response.changed()
}

/// A word in a pill, for a style's settings read as a list of facts.
pub(crate) fn tag(ui: &mut Ui, text: &str) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::proportional(Theme::TYPE_SM),
        Theme::text_primary(),
    );
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(14.0, 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, rect.height() / 2.0, Theme::selected_bg());
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        Theme::text_primary(),
    );
}

/// A colour as a tile to choose: the colour in a rounded block, its name
/// under it.
pub(crate) fn swatch_tile(ui: &mut Ui, rgba: [f32; 4], name: &str, chosen: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(78.0, 64.0), Sense::click());
    let painter = ui.painter();
    if chosen {
        painter.rect_filled(rect, 8.0, Theme::accent_soft());
    } else if response.hovered() {
        painter.rect_filled(rect, 8.0, Theme::hover_bg());
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }
    let block = Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 22.0),
        Vec2::new(54.0, 30.0),
    );
    painter.rect_filled(block, 6.0, Theme::panel_bg_solid());
    painter.rect_filled(block, 6.0, srgb(rgba));
    painter.rect_stroke(
        block,
        6.0,
        Stroke::new(
            if chosen { 2.0 } else { 1.0 },
            if chosen {
                Theme::accent()
            } else {
                Theme::border()
            },
        ),
        egui::StrokeKind::Outside,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        FontId::proportional(Theme::TYPE_SM),
        Theme::text_primary(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 6.0);
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            block.bottom() + 6.0,
        ),
        galley,
        if chosen {
            Theme::text_primary()
        } else {
            Theme::text_muted()
        },
    );
    crate::icons::reads_as(
        response,
        name,
        egui::WidgetType::SelectableLabel,
        Some(chosen),
    )
    .on_hover_text(name)
}

/// A colour's numbers as egui draws them: sRGB, which is what the page
/// draws them as too.
pub(crate) fn srgb([r, g, b, a]: [f32; 4]) -> Color32 {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(byte(r), byte(g), byte(b), byte(a))
}

/// A card whose title row carries an action at its right.
pub(crate) fn card_with_action<R>(
    ui: &mut Ui,
    title: &str,
    action: impl FnOnce(&mut Ui),
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    card(ui, None, |ui| {
        ui.horizontal(|ui| {
            overline(ui, title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), action);
        });
        ui.add_space(2.0);
        add(ui)
    })
}

/// A number worth reading at a glance, large, with what it counts under it.
pub(crate) fn stat(ui: &mut Ui, value: usize, counts: &str) -> Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(
            egui::Label::new(
                egui::RichText::new(value.to_string())
                    .font(heading_font(22.0))
                    .color(Theme::text_primary()),
            )
            .selectable(false),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(counts)
                    .size(Theme::TYPE_SM)
                    .color(Theme::text_muted()),
            )
            .selectable(false),
        );
    })
    .response
}

/// A page's picture and name as a link to it, for the style's settings
/// read as a list.
pub(crate) fn page_link(ui: &mut Ui, icon: Icon, title: &str, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 5.0, Theme::hover_bg());
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            5.0,
            Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }
    let glyph = Rect::from_min_size(
        egui::pos2(rect.left() + 2.0, rect.center().y - Theme::ICON_SIZE / 2.0),
        Vec2::splat(Theme::ICON_SIZE),
    );
    crate::icons::paint(painter, glyph, icon, Theme::text_muted());
    let mut job = egui::text::LayoutJob::simple_singleline(
        title.to_owned(),
        FontId::proportional(Theme::TYPE_MD),
        Theme::text_primary(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width - Theme::ICON_SIZE - 10.0);
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(glyph.right() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        if response.hovered() {
            Theme::accent_hover()
        } else {
            Theme::text_primary()
        },
    );
    crate::icons::reads_as(response, title, egui::WidgetType::Link, None)
        .on_hover_text("Go to this page")
}

/// The ground the preview's paper lies on: a step deeper than a card in the
/// light palette, where a card is itself white and the paper would vanish
/// into it, and a card's own in the dark one.
pub(crate) fn well_fill() -> Color32 {
    if Theme::is_light() {
        crate::theme::palette().step(3)
    } else {
        card_fill()
    }
}

/// Set a slider for this window: a rail that shows on a card in either
/// palette, filled in the accent up to the handle so the amount reads as a
/// length. egui draws the rail in a field's colour, which on a white card
/// is no rail at all.
pub(crate) fn slider_look(ui: &mut Ui) {
    let visuals = ui.visuals_mut();
    visuals.widgets.inactive.bg_fill = Theme::selected_bg();
    visuals.slider_trailing_fill = true;
    visuals.selection.bg_fill = Theme::accent();
    ui.spacing_mut().slider_width = 180.0;
}
