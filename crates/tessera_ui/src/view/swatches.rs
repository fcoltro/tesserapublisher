//! The Swatches panel: the document's named colours.
//!
//! A swatch is not a colour you copied — it is a colour objects *refer to*. The
//! panel is therefore not a palette of things to pick from but a list of the
//! document's own definitions: rename one and every object follows, edit one and
//! every object changes, delete one and the objects using it say so rather than
//! quietly keeping its last value.
//!
//! That last point is why the delete button reports a count. "Remove Brand red"
//! is a different decision when four objects use it than when none do, and a
//! panel that does not say which is asking somebody to guess.

use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};

use tessera_color::Color;
use tessera_document::nodes::Swatch;
use tessera_document::paint::Paint;

use super::swatch_editor::{self, BLACK, BLACK_INK, NONE, PAPER, PAPER_INK};
use crate::app::{SwatchTarget, TesseraApp};
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// A row's height: a chip with air round it, and a name that reads.
const ROW: f32 = 26.0;
/// The colour chip on each row.
const CHIP: Vec2 = Vec2::new(26.0, 16.0);
/// Past this many swatches the list gets a filter: a document imported from
/// InDesign can bring fifty, and a list of fifty is read by searching it.
const FILTER_FROM: usize = 8;

/// The section, as it sits in the rail.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    body(ui, state);
}

/// A colour the list offers, as it is applied: a built-in, or a swatch by
/// name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Entry {
    None,
    Paper,
    Black,
    Named(String),
}

impl Entry {
    /// The entry the panel lists under `name`.
    pub(crate) fn from_name(name: &str) -> Self {
        match name {
            NONE => Self::None,
            PAPER => Self::Paper,
            BLACK => Self::Black,
            other => Self::Named(other.to_owned()),
        }
    }

    pub(crate) fn name(&self) -> &str {
        match self {
            Self::None => NONE,
            Self::Paper => PAPER,
            Self::Black => BLACK,
            Self::Named(name) => name,
        }
    }

    /// What it puts on an object: nothing, for [None]; a swatch by
    /// reference, so editing the swatch recolours everything it is on.
    fn colour(&self) -> Option<Color> {
        match self {
            Self::None => None,
            Self::Paper => Some(PAPER_INK),
            Self::Black => Some(BLACK_INK),
            Self::Named(name) => Some(Color::Swatch {
                name: name.clone(),
                tint: 1.0,
            }),
        }
    }

    /// The entry a colour on the page is, if it is one of the list's.
    fn of(colour: &Color) -> Option<Self> {
        match colour {
            Color::Swatch { name, .. } => Some(Self::Named(name.clone())),
            c if *c == PAPER_INK => Some(Self::Paper),
            c if *c == BLACK_INK => Some(Self::Black),
            c if c.to_rgb_f32()[3] <= 0.0 => Some(Self::None),
            _ => None,
        }
    }
}

/// What Apply would do with `entry` now, or `None` when the selection has
/// nothing for it to colour: no object for a fill or a stroke, no text for
/// text, and [None] for text, which has to be some colour.
pub(crate) fn apply_commands(
    state: &TesseraApp,
    entry: &Entry,
    target: SwatchTarget,
) -> Option<Vec<Command>> {
    let colour = entry.colour();
    match target {
        SwatchTarget::Fill | SwatchTarget::Stroke => {
            let frames = state.active().selection.as_slice();
            if frames.is_empty() {
                return None;
            }
            let doc = state.active().document();
            Some(
                frames
                    .iter()
                    .map(|id| match (target, colour.clone()) {
                        (SwatchTarget::Fill, None) => Command::ClearFill(*id),
                        (SwatchTarget::Fill, Some(colour)) => Command::SetFill {
                            id: *id,
                            paint: Paint::Solid(colour),
                        },
                        (_, None) => Command::SetStroke {
                            id: *id,
                            stroke: None,
                        },
                        // The stroke's colour, and nothing else about it: a
                        // swatch is a colour, and the weight, the dashes and
                        // the corners are somebody's other decisions.
                        (_, Some(colour)) => Command::SetStroke {
                            id: *id,
                            stroke: Some(
                                doc.frame(*id).and_then(|f| f.stroke.clone()).map_or_else(
                                    || tessera_document::nodes::Stroke::new(colour.clone(), 1.0),
                                    |stroke| tessera_document::nodes::Stroke {
                                        color: colour.clone(),
                                        ..stroke
                                    },
                                ),
                            ),
                        },
                    })
                    .collect(),
            )
        }
        SwatchTarget::Text => {
            let colour = colour?;
            let (story, range) = super::panels::text_in_hand(state)?;
            Some(vec![Command::SetCharacterFormat {
                story,
                range,
                format: tessera_text::story::CharacterFormat {
                    colour: Some(colour),
                    ..Default::default()
                },
            }])
        }
    }
}

/// Colour the selection with `entry`, if it has anything to colour.
pub(crate) fn apply_entry(state: &mut TesseraApp, entry: &Entry) {
    let target = state.swatches_window.target;
    if let Some(commands) = apply_commands(state, entry, target) {
        for command in commands {
            apply(state, command);
        }
    }
}

/// Why Apply cannot act, for its tooltip.
pub(crate) fn apply_hint(target: SwatchTarget) -> &'static str {
    match target {
        SwatchTarget::Fill => "Select an object to fill it",
        SwatchTarget::Stroke => "Select an object to stroke it",
        SwatchTarget::Text => "Select some text, or a text frame, to colour it",
    }
}

/// Fill, stroke or text: what Apply colours.
pub(crate) fn target_choice(ui: &mut Ui, state: &mut TesseraApp) {
    let mut target = state.swatches_window.target;
    if super::style_ui::segmented(
        ui,
        "Apply to",
        &mut target,
        &[
            (super::style_ui::Segment::Text("Fill"), SwatchTarget::Fill),
            (
                super::style_ui::Segment::Text("Stroke"),
                SwatchTarget::Stroke,
            ),
            (super::style_ui::Segment::Text("Text"), SwatchTarget::Text),
        ],
        false,
    ) {
        state.swatches_window.target = target;
    }
}

/// The entry the selection's fill, stroke or text is coloured with now, so
/// the list can say which it is — the question "which swatch is this?"
/// answered without opening anything.
fn on_selection(state: &TesseraApp) -> Option<Entry> {
    let doc = state.active().document();
    match state.swatches_window.target {
        SwatchTarget::Fill => {
            let frame = doc.frame(state.active().selection.single()?)?;
            match &frame.fill {
                Paint::Solid(colour) => Entry::of(colour),
                Paint::Gradient(_) => None,
            }
        }
        SwatchTarget::Stroke => {
            let frame = doc.frame(state.active().selection.single()?)?;
            match &frame.stroke {
                None => Some(Entry::None),
                Some(stroke) => Entry::of(&stroke.color),
            }
        }
        SwatchTarget::Text => {
            let (story, range) = super::panels::text_in_hand(state)?;
            let colour = doc.story(story)?.common_format_local(range).colour?;
            Entry::of(&colour)
        }
    }
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    let swatches = state.active().document().swatches.clone();
    let counts = swatch_editor::use_counts(state);
    let current = on_selection(state);

    if super::panel_ui::action(ui, Icon::Plus, "New swatch")
        .on_hover_text("Name the selected object's fill, or a plain black")
        .clicked()
    {
        let swatch = fresh(state);
        let window = &mut state.swatches_window;
        window.chosen = Some(swatch.name.clone());
        window.editing = true;
        apply(state, Command::SetSwatch(swatch));
    }
    ui.add_space(Theme::space_2());
    ui.horizontal(|ui| {
        ui.colored_label(Theme::text_muted(), "Apply to");
        target_choice(ui, state);
    });
    ui.add_space(Theme::space_2());

    if swatches.len() > FILTER_FROM {
        ui.add(
            egui::TextEdit::singleline(&mut state.swatches_window.filter)
                .hint_text("Filter swatches")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(Theme::space_1());
    }
    let filter = state.swatches_window.filter.trim().to_lowercase();

    let chosen = state.swatches_window.chosen.clone();
    let mut choose: Option<Entry> = None;
    let mut open: Option<String> = None;
    let mut menu: Option<(String, MenuAction)> = None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        for entry in [Entry::None, Entry::Paper, Entry::Black] {
            let shown = entry.colour().map(|c| c.to_rgb_f32());
            let response = row(
                ui,
                &Row {
                    name: entry.name(),
                    shown,
                    spot: false,
                    badge: None,
                    count: None,
                    chosen: chosen.as_deref() == Some(entry.name()),
                    current: current.as_ref() == Some(&entry),
                },
            )
            .on_hover_text("Built in: always here, and not edited");
            if response.clicked() {
                choose = Some(entry);
            }
        }
        ui.add_space(3.0);
        let rule = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            rule,
            Stroke::new(1.0, Theme::rule()),
        );
        ui.add_space(4.0);

        let doc = state.active().document();
        for swatch in swatches
            .iter()
            .filter(|s| filter.is_empty() || s.name.to_lowercase().contains(&filter))
        {
            let entry = Entry::Named(swatch.name.clone());
            let shown = doc
                .resolve_colour(&Color::Swatch {
                    name: swatch.name.clone(),
                    tint: 1.0,
                })
                .to_rgb_f32();
            let places = counts
                .iter()
                .find(|(name, _)| *name == swatch.name)
                .map_or(0, |(_, n)| *n);
            let is_chosen = chosen.as_deref() == Some(swatch.name.as_str());
            let response = ui.push_id(&swatch.name, |ui| {
                row(
                    ui,
                    &Row {
                        name: &swatch.name,
                        shown: Some(shown),
                        spot: swatch_editor::is_spot(swatch),
                        badge: Some(badge(swatch)),
                        count: Some(places),
                        chosen: is_chosen,
                        current: current.as_ref() == Some(&entry),
                    },
                )
            });
            let response = response
                .inner
                .on_hover_text(format!("{} Double-click to edit.", use_sentence(places)));
            if response.clicked() {
                choose = Some(entry.clone());
            }
            if response.double_clicked() {
                open = Some(swatch.name.clone());
            }
            response.context_menu(|ui| {
                for action in MenuAction::ALL {
                    if action == MenuAction::NewTint
                        && matches!(swatch.colour, Color::Swatch { .. })
                    {
                        continue;
                    }
                    if ui.button(action.label()).clicked() {
                        menu = Some((swatch.name.clone(), action));
                        ui.close();
                    }
                }
            });
            if is_chosen {
                // The numbers, under the row being worked on: enough to read
                // a colour off without opening it.
                ui.horizontal(|ui| {
                    ui.add_space(CHIP.x + 12.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(swatch_editor::numbers(&swatch.colour))
                                .size(Theme::TYPE_SM)
                                .color(Theme::text_muted()),
                        )
                        .truncate()
                        .selectable(false),
                    );
                });
            }
        }
    });
    if swatches.is_empty() {
        super::panel_ui::empty(
            ui,
            "No named colours yet",
            "Create a swatch from the selected object's fill, then reuse it throughout your document.",
        );
    } else if !filter.is_empty()
        && !swatches
            .iter()
            .any(|s| s.name.to_lowercase().contains(&filter))
    {
        super::panel_ui::hint(ui, "No swatch has that in its name.");
    }

    ui.add_space(Theme::space_2());
    actions(ui, state);

    if let Some(entry) = choose {
        state.swatches_window.chosen = Some(entry.name().to_owned());
    }
    if let Some(name) = open {
        state.swatches_window.chosen = Some(name);
        state.swatches_window.editing = true;
    }
    if let Some((name, action)) = menu {
        state.swatches_window.chosen = Some(name.clone());
        run(state, &name, action);
    }
}

/// "Used by nothing yet." or "Used in 4 places."
fn use_sentence(places: usize) -> String {
    match places {
        0 => "Nothing uses it yet.".to_string(),
        1 => "Used in 1 place.".to_string(),
        n => format!("Used in {n} places."),
    }
}

/// The space a swatch is written in, or that it is a tint.
fn badge(swatch: &Swatch) -> &'static str {
    swatch_editor::Mode::of(&swatch.colour).map_or("Tint", swatch_editor::Mode::label)
}

/// What a row shows.
struct Row<'a> {
    name: &'a str,
    /// The colour, or `None` for [None].
    shown: Option<[f32; 4]>,
    spot: bool,
    badge: Option<&'a str>,
    /// How many places use it; `None` for a built-in, which is not counted.
    count: Option<usize>,
    chosen: bool,
    /// Whether the selection's fill, stroke or text is this now.
    current: bool,
}

/// One entry of the list: its colour, its name, the space it is written in
/// and how many places use it — the chosen one on the accent's ground, and
/// the one the selection is coloured with marked with a dot.
fn row(ui: &mut Ui, row: &Row<'_>) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let painter = ui.painter_at(rect.expand(1.0));
    if row.chosen {
        painter.rect_filled(rect, Theme::RADIUS, Theme::accent_soft());
    } else if response.hovered() {
        painter.rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            Theme::RADIUS,
            Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }
    let chip = Rect::from_min_size(
        egui::pos2(rect.left() + 6.0, rect.center().y - CHIP.y / 2.0),
        CHIP,
    );
    match row.shown {
        Some(rgba) => swatch_editor::chip(&painter, chip, rgba, 4.0, row.spot),
        None => {
            painter.rect_filled(chip, 4.0, Color32::WHITE);
            painter.line_segment(
                [chip.left_bottom(), chip.right_top()],
                Stroke::new(1.5, Color32::from_rgb(0xE0, 0x30, 0x30)),
            );
            painter.rect_stroke(
                chip,
                4.0,
                Stroke::new(1.0, Theme::border()),
                egui::StrokeKind::Inside,
            );
        }
    }

    // From the right: the count, the badge, the dot; the name takes the rest.
    let mut right = rect.right() - 6.0;
    let small = egui::FontId::proportional(Theme::TYPE_SM);
    match row.count {
        Some(count) => {
            let galley =
                painter.layout_no_wrap(count.to_string(), small.clone(), Theme::text_muted());
            let x = right - galley.size().x.max(14.0);
            painter.galley(
                egui::pos2(
                    right - galley.size().x,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                if count == 0 {
                    Theme::rule()
                } else {
                    Theme::text_muted()
                },
            );
            right = x - 8.0;
        }
        None => {
            let lock = Rect::from_center_size(
                egui::pos2(right - 7.0, rect.center().y),
                Vec2::splat(Theme::ICON_SIZE - 4.0),
            );
            crate::icons::paint(&painter, lock, Icon::Lock, Theme::rule());
            right = lock.left() - 8.0;
        }
    }
    if let Some(badge) = row.badge {
        let galley = painter.layout_no_wrap(
            badge.to_owned(),
            egui::FontId::proportional(Theme::TYPE_SM - 1.5),
            Theme::text_muted(),
        );
        let pill = Rect::from_min_max(
            egui::pos2(right - galley.size().x - 10.0, rect.center().y - 8.0),
            egui::pos2(right, rect.center().y + 8.0),
        );
        painter.rect_stroke(
            pill,
            8.0,
            Stroke::new(1.0, Theme::rule()),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            pill.center() - galley.size() / 2.0,
            galley,
            Theme::text_muted(),
        );
        right = pill.left() - 6.0;
    }
    if row.current {
        painter.circle_filled(
            egui::pos2(right - 3.0, rect.center().y),
            3.5,
            Theme::accent(),
        );
        right -= 12.0;
    }
    let left = chip.right() + 8.0;
    let mut job = egui::text::LayoutJob::simple_singleline(
        row.name.to_owned(),
        egui::FontId::proportional(Theme::TYPE_MD),
        Theme::text_primary(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(12.0));
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        Theme::text_primary(),
    );
    let label = if row.current {
        format!("{}, on the selection", row.name)
    } else {
        row.name.to_owned()
    };
    let response = crate::icons::reads_as(
        response,
        row.name,
        egui::WidgetType::SelectableLabel,
        Some(row.chosen),
    );
    if row.current {
        response.on_hover_text(label)
    } else {
        response
    }
}

/// What a swatch's menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuAction {
    Edit,
    NewTint,
    Duplicate,
    Delete,
}

impl MenuAction {
    const ALL: [Self; 4] = [Self::Edit, Self::NewTint, Self::Duplicate, Self::Delete];

    fn label(self) -> &'static str {
        match self {
            Self::Edit => "Edit swatch…",
            Self::NewTint => "New tint swatch",
            Self::Duplicate => "Duplicate swatch",
            Self::Delete => "Delete swatch…",
        }
    }
}

fn run(state: &mut TesseraApp, name: &str, action: MenuAction) {
    match action {
        MenuAction::Edit => state.swatches_window.editing = true,
        MenuAction::NewTint => {
            swatch_editor::new_tint(state, name, 0.5);
            state.swatches_window.editing = true;
        }
        MenuAction::Duplicate => {
            let Some(swatch) = state.active().document().swatch(name).cloned() else {
                return;
            };
            let copy = swatch_editor::unused_name(state, &format!("{name} copy"));
            apply(
                state,
                Command::SetSwatch(Swatch {
                    name: copy.clone(),
                    ..swatch
                }),
            );
            state.swatches_window.chosen = Some(copy);
        }
        MenuAction::Delete => swatch_editor::delete(state, name),
    }
}

/// What can be done with the chosen entry: apply it, and for a swatch of
/// the document's own, open it, make a tint of it, or delete it.
fn actions(ui: &mut Ui, state: &mut TesseraApp) {
    let entry = state
        .swatches_window
        .chosen
        .as_deref()
        .map(Entry::from_name)
        .filter(|entry| match entry {
            Entry::Named(name) => state.active().document().swatch(name).is_some(),
            _ => true,
        });
    let target = state.swatches_window.target;
    let ready = entry
        .as_ref()
        .is_some_and(|entry| apply_commands(state, entry, target).is_some());
    let named = match &entry {
        Some(Entry::Named(name)) => state.active().document().swatch(name).cloned(),
        _ => None,
    };
    let mut apply_now = false;
    let mut action = None;
    ui.horizontal(|ui| {
        apply_now = ui
            .add_enabled(ready, super::primary_button("Apply"))
            .on_hover_text(match target {
                SwatchTarget::Fill => "Fill the selection with the chosen colour",
                SwatchTarget::Stroke => "Stroke the selection with the chosen colour",
                SwatchTarget::Text => "Colour the selected text with the chosen colour",
            })
            .on_disabled_hover_text(if entry.is_none() {
                "Choose a colour in the list first"
            } else if entry == Some(Entry::None) && target == SwatchTarget::Text {
                "Text has to be some colour"
            } else {
                apply_hint(target)
            })
            .clicked();
        if let Some(swatch) = &named {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if super::panels::icon_button(ui, Icon::Trash, "Delete swatch…", false) {
                    action = Some(MenuAction::Delete);
                }
                if super::panels::icon_button(ui, Icon::Duplicate, "Duplicate swatch", false) {
                    action = Some(MenuAction::Duplicate);
                }
                if !matches!(swatch.colour, Color::Swatch { .. })
                    && super::panels::icon_button(ui, Icon::Tint, "New tint swatch", false)
                {
                    action = Some(MenuAction::NewTint);
                }
                if super::panels::icon_button(ui, Icon::Edit, "Edit swatch…", false) {
                    action = Some(MenuAction::Edit);
                }
            });
        }
    });
    if apply_now && let Some(entry) = &entry {
        apply_entry(state, entry);
    }
    if let (Some(action), Some(swatch)) = (action, named) {
        run(state, &swatch.name, action);
    }
}

/// A new swatch, named so as not to collide.
///
/// It takes the selected object's fill when there is one, because naming the
/// colour you are looking at is what "new swatch" almost always means. With
/// nothing selected it is a plain black, which is a colour rather than a
/// surprise.
///
/// The colour the fill *is*, not what it names: a fill that is already a
/// swatch would otherwise make a second name for the first — a 100% tint
/// nobody asked for — and a spot's process stand-in rather than the ink, so
/// the new swatch is not a second plate of the same name. No fill at all is
/// no colour to name, and makes the black.
fn fresh(state: &TesseraApp) -> Swatch {
    let doc = state.active().document();
    let colour = state
        .active()
        .selection
        .single()
        .and_then(|id| doc.frame(id))
        .map(
            |frame| match doc.resolve_colour(&frame.fill.representative()) {
                Color::Spot { fallback, .. } => *fallback,
                other => other,
            },
        )
        .filter(|colour| !colour.is_reference() && colour.to_rgb_f32()[3] > 0.0)
        .unwrap_or(Color::BLACK);

    let taken: Vec<&str> = state
        .active()
        .document()
        .swatches
        .iter()
        .map(|s| s.name.as_str())
        .collect();

    let mut n = 1;
    let name = loop {
        let candidate = format!("Colour {n}");
        if !taken.contains(&candidate.as_str()) {
            break candidate;
        }
        n += 1;
    };

    Swatch {
        name,
        colour,
        spot: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_swatch_takes_the_selected_objects_colour() {
        // Naming the colour you are looking at is what "new swatch" almost
        // always means.
        let mut state = TesseraApp::headless();
        let teal = Color::Rgb {
            r: 0.0,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: tessera_document::paint::Paint::Solid(teal.clone()),
            },
        );

        assert_eq!(fresh(&state).colour, teal);
    }

    #[test]
    fn the_built_in_black_is_solid_k_and_paper_is_no_ink() {
        // What a press means by them; RGB white and a four-colour black would
        // separate onto plates they have no business on.
        let black = Color::Cmyk {
            c: 0.0,
            m: 0.0,
            y: 0.0,
            k: 1.0,
            a: 1.0,
        };
        let [r, g, b, _] = black.to_rgb_f32();
        assert!(
            r < 0.3 && g < 0.3 && b < 0.3,
            "black reads dark: {r} {g} {b}"
        );
        let paper = Color::Cmyk {
            c: 0.0,
            m: 0.0,
            y: 0.0,
            k: 0.0,
            a: 1.0,
        };
        let [r, g, b, _] = paper.to_rgb_f32();
        assert!(r > 0.95 && g > 0.95 && b > 0.95, "paper reads white");
    }

    #[test]
    fn a_new_swatch_with_nothing_selected_is_a_colour_rather_than_a_surprise() {
        let state = TesseraApp::headless();
        assert_eq!(fresh(&state).colour, Color::BLACK);
    }

    #[test]
    fn a_new_swatch_never_takes_a_name_already_in_use() {
        // Two swatches of one name would be two colours, and objects would
        // silently take whichever came first.
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            let swatch = fresh(&state);
            apply(&mut state, Command::SetSwatch(swatch));
        }

        let names: Vec<String> = state
            .active()
            .document()
            .swatches
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names.len(), 3, "got {names:?}");
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 3, "a name was reused: {names:?}");
    }

    #[test]
    fn renaming_a_swatch_keeps_object_references_and_undoes_in_one_step() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::SetSwatch(Swatch::new("Brand", Color::BLACK)),
        );
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().unwrap();
        let reference = |name: &str| {
            tessera_document::paint::Paint::Solid(Color::Swatch {
                name: name.into(),
                tint: 0.6,
            })
        };
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: reference("Brand"),
            },
        );
        apply(
            &mut state,
            Command::EditSwatch {
                old: "Brand".into(),
                swatch: Swatch::new("Ink", Color::BLACK),
            },
        );
        assert_eq!(state.active().document().frames[id].fill, reference("Ink"));
        assert!(state.active().document().swatch("Brand").is_none());
        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().frames[id].fill,
            reference("Brand")
        );
        assert!(state.active().document().swatch("Brand").is_some());
        assert!(state.active().document().swatch("Ink").is_none());
    }

    fn a_rectangle(state: &mut TesseraApp) -> tessera_document::ids::FrameId {
        apply(
            state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        state.active().selection.single().expect("selected")
    }

    fn brand() -> Color {
        Color::Swatch {
            name: "Brand".into(),
            tint: 1.0,
        }
    }

    fn with_brand() -> TesseraApp {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::SetSwatch(Swatch::new("Brand", Color::BLACK)),
        );
        state
    }

    #[test]
    fn a_stroke_takes_the_colour_and_keeps_its_weight_and_dashes() {
        // A swatch is a colour; the weight and the dashes are somebody's
        // other decisions.
        let mut state = with_brand();
        let id = a_rectangle(&mut state);
        let mut stroke = tessera_document::nodes::Stroke::new(Color::WHITE, 4.0);
        stroke.dashes = vec![3.0, 2.0];
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(stroke.clone()),
            },
        );
        state.swatches_window.target = SwatchTarget::Stroke;
        apply_entry(&mut state, &Entry::Named("Brand".into()));
        let now = state.active().document().frames[id]
            .stroke
            .clone()
            .expect("a stroke");
        assert_eq!(now.color, brand());
        assert_eq!((now.width, now.dashes), (4.0, vec![3.0, 2.0]));

        apply_entry(&mut state, &Entry::None);
        assert!(
            state.active().document().frames[id].stroke.is_none(),
            "[None] takes it off"
        );
    }

    #[test]
    fn text_takes_the_colour_of_the_text_in_hand() {
        let mut state = with_brand();
        apply(
            &mut state,
            Command::AddTextFrame(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "Words".into(),
            },
        );
        state.swatches_window.target = SwatchTarget::Text;
        assert!(
            apply_commands(&state, &Entry::None, SwatchTarget::Text).is_none(),
            "text has to be some colour"
        );
        apply_entry(&mut state, &Entry::Named("Brand".into()));
        let tessera_document::nodes::FrameKind::Text { story, .. } =
            state.active().document().frames[id].kind
        else {
            panic!("a text frame");
        };
        let story = state.active().document().story(story).expect("story");
        assert_eq!(story.common_format_local(0..5).colour, Some(brand()));
        assert_eq!(on_selection(&state), Some(Entry::Named("Brand".into())));
    }

    #[test]
    fn nothing_selected_is_nothing_to_apply_to() {
        let state = with_brand();
        for target in [SwatchTarget::Fill, SwatchTarget::Stroke, SwatchTarget::Text] {
            assert!(apply_commands(&state, &Entry::Named("Brand".into()), target).is_none());
        }
    }

    #[test]
    fn the_list_marks_the_swatch_the_selection_is_coloured_with() {
        let mut state = with_brand();
        let id = a_rectangle(&mut state);
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: Paint::Solid(brand()),
            },
        );
        assert_eq!(on_selection(&state), Some(Entry::Named("Brand".into())));
        apply(&mut state, Command::ClearFill(id));
        assert_eq!(on_selection(&state), Some(Entry::None));
        state.swatches_window.target = SwatchTarget::Stroke;
        apply(&mut state, Command::SetStroke { id, stroke: None });
        assert_eq!(
            on_selection(&state),
            Some(Entry::None),
            "no stroke is [None]"
        );
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(tessera_document::nodes::Stroke::new(
                    swatch_editor::BLACK_INK,
                    1.0,
                )),
            },
        );
        assert_eq!(on_selection(&state), Some(Entry::Black));
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(tessera_document::nodes::Stroke::new(Color::BLACK, 1.0)),
            },
        );
        assert_eq!(
            on_selection(&state),
            None,
            "four-colour black is not [Black], and no row claims it"
        );
    }

    #[test]
    fn a_new_swatch_from_a_fill_that_is_already_one_is_its_colour_not_a_second_name() {
        let mut state = TesseraApp::headless();
        let red = Color::Cmyk {
            c: 0.0,
            m: 0.9,
            y: 0.8,
            k: 0.0,
            a: 1.0,
        };
        apply(
            &mut state,
            Command::SetSwatch(Swatch::new("Brand", red.clone())),
        );
        let id = a_rectangle(&mut state);
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: Paint::Solid(brand()),
            },
        );
        assert_eq!(fresh(&state).colour, red);
    }

    /// One frame of the panel, answering with what a screen reader would be
    /// told.
    fn panel(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let output = crate::headless_frame::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| docked(ui, state),
        );
        output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        Some((
                            node.label()?.to_string(),
                            egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn press_on(ctx: &egui::Context, state: &mut TesseraApp, label: &str, times: usize) {
        let nodes = panel(ctx, state, Vec::new());
        let at = nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
            .center();
        for _ in 0..times {
            for pressed in [true, false] {
                panel(
                    ctx,
                    state,
                    vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                );
            }
        }
    }

    fn a_panel() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    #[test]
    fn a_click_chooses_and_a_double_click_opens_the_swatch_window() {
        let mut state = with_brand();
        let ctx = a_panel();
        press_on(&ctx, &mut state, "Brand", 1);
        assert_eq!(state.swatches_window.chosen.as_deref(), Some("Brand"));
        assert!(!state.swatches_window.editing, "choosing is not editing");
        press_on(&ctx, &mut state, "Brand", 2);
        assert!(state.swatches_window.editing);
    }

    #[test]
    fn a_click_chooses_a_built_in_and_apply_puts_it_on() {
        // Choosing and applying are separate: browsing the list with an
        // object selected must not recolour it swatch by swatch.
        let mut state = with_brand();
        let id = a_rectangle(&mut state);
        let ctx = a_panel();
        press_on(&ctx, &mut state, "[Black]", 1);
        assert_ne!(
            state.active().document().frames[id].fill,
            Paint::Solid(swatch_editor::BLACK_INK),
            "not yet"
        );
        press_on(&ctx, &mut state, "Apply", 1);
        assert_eq!(
            state.active().document().frames[id].fill,
            Paint::Solid(swatch_editor::BLACK_INK)
        );
    }

    #[test]
    fn a_long_list_can_be_narrowed_by_name() {
        let mut state = TesseraApp::headless();
        for n in 0..10 {
            apply(
                &mut state,
                Command::SetSwatch(Swatch::new(format!("Grey {n}"), Color::BLACK)),
            );
        }
        apply(
            &mut state,
            Command::SetSwatch(Swatch::new("Brand", Color::BLACK)),
        );
        state.swatches_window.filter = "bra".into();
        let ctx = a_panel();
        panel(&ctx, &mut state, Vec::new());
        let shown = panel(&ctx, &mut state, Vec::new());
        assert!(shown.iter().any(|(name, _)| name == "Brand"));
        assert!(!shown.iter().any(|(name, _)| name.starts_with("Grey")));
    }
}
