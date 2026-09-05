//! The pages panel: which spreads exist, which one you are on, and reordering.
//!
//! Thumbnails are **schematic**, drawn with egui's painter from the resolved
//! document: each frame becomes a filled rectangle in its own colour. A real
//! thumbnail would mean rendering every spread to a texture and keeping those
//! textures in step with the document, which is a cache with an invalidation
//! rule — and at the size these are drawn, the schematic says the same thing.
//! It also costs no GPU and cannot fall behind, because it is redrawn from the
//! same resolve the canvas uses.

use egui::Ui;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// How wide a spread's thumbnail is drawn, in screen points.
const THUMBNAIL: f32 = 96.0;

/// The window, if it is open.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    if !state.pages_window.open {
        return;
    }

    let mut open = true;
    egui::Window::new("Pages")
        .open(&mut open)
        .default_width(160.0)
        .default_height(420.0)
        .vscroll(true)
        .show(ui.ctx(), |ui| body(ui, state));
    state.pages_window.open = open;
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    let spreads = state.active().document().spread_order.clone();
    let current = state
        .active()
        .current_spread
        .min(spreads.len().saturating_sub(1));

    // Where a dragged spread should land, decided while drawing and acted on
    // afterwards — moving the list mid-iteration would renumber what is still
    // being drawn.
    let mut drop: Option<(usize, usize)> = None;
    let mut turn_to: Option<usize> = None;

    for (index, spread) in spreads.iter().enumerate() {
        let id = egui::Id::new(("spread", index));
        let (response, payload) = ui.dnd_drop_zone::<usize, _>(egui::Frame::NONE, |ui| {
            ui.dnd_drag_source(id, index, |ui| {
                thumbnail(ui, state, *spread, index, index == current);
            });
        });
        if let Some(from) = payload {
            drop = Some((*from, index));
        }
        if response.response.clicked() {
            turn_to = Some(index);
        }
    }

    if let Some(at) = turn_to {
        state.active_mut().current_spread = at;
        state.active_mut().fitted = false;
    }
    if let Some((from, to)) = drop
        && from != to
    {
        apply(state, Command::MoveSpread { from, to });
        // Follow the spread that moved, so the panel does not silently change
        // which one is being looked at.
        state.active_mut().current_spread = to;
    }

    ui.separator();
    ui.horizontal(|ui| {
        if crate::view::panels::icon_button(ui, crate::icons::Icon::Plus, "Add page", false) {
            apply(state, Command::AddPage);
        }
        if let Some(page) = crate::view::panels::current_page(state) {
            if crate::view::panels::icon_button(
                ui,
                crate::icons::Icon::Duplicate,
                "Duplicate this page",
                false,
            ) {
                apply(state, Command::DuplicatePage { id: page });
            }
            if crate::view::panels::icon_button(
                ui,
                crate::icons::Icon::Trash,
                "Delete this page",
                false,
            ) {
                apply(state, Command::RemovePage { id: page });
            }
        }
    });
}

/// One spread, drawn as its pages with their contents blocked in.
fn thumbnail(
    ui: &mut Ui,
    state: &TesseraApp,
    spread: tessera_document::ids::SpreadId,
    index: usize,
    current: bool,
) {
    let doc = state.active().document();
    let pages = doc.pages_of(spread);
    let Some(first) = pages.first().and_then(|p| doc.pages.get(*p)) else {
        return;
    };

    // The spread's own box in document space, so the thumbnail is the right
    // shape whatever the page size and however many pages face each other.
    let width = first.bounds.width * pages.len() as f64;
    let height = first.bounds.height;
    let scale = f64::from(THUMBNAIL) / width.max(1.0);

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(THUMBNAIL, (height * scale) as f32),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 1.0, egui::Color32::WHITE);

    // Every frame on the spread, as a rectangle in its own fill. Read from the
    // document rather than the resolve cache, because this draws inside a
    // window and the cache belongs to the canvas — asking for it here would
    // resolve a second time in the same frame.
    for page in &pages {
        let Some(page) = doc.pages.get(*page) else {
            continue;
        };
        for layer in &page.layers {
            let Some(layer) = doc.layers.get(*layer) else {
                continue;
            };
            if !layer.visible {
                continue;
            }
            for frame in &layer.frames {
                let Some(frame) = doc.frame(*frame) else {
                    continue;
                };
                let b = frame.bounds;
                let at = egui::Rect::from_min_size(
                    rect.min
                        + egui::vec2(
                            ((b.x - first.bounds.x) * scale) as f32,
                            ((b.y - first.bounds.y) * scale) as f32,
                        ),
                    egui::vec2((b.width * scale) as f32, (b.height * scale) as f32),
                );
                let [r, g, bl, a] = frame.fill.to_rgb_f32();
                painter.rect_filled(
                    at.intersect(rect),
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(
                        (r * 255.0) as u8,
                        (g * 255.0) as u8,
                        (bl * 255.0) as u8,
                        // Never fully transparent: a text frame's fill is
                        // clear by default, and a thumbnail showing nothing
                        // where something is would be a lie.
                        ((a * 255.0) as u8).max(60),
                    ),
                );
            }
        }
    }

    painter.rect_stroke(
        rect,
        1.0,
        egui::Stroke::new(
            if current { 2.0 } else { 1.0 },
            if current {
                Theme::ACCENT
            } else {
                Theme::BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );

    // The page numbers this spread holds, under it.
    let numbers = page_numbers(state, spread);
    ui.colored_label(
        if current {
            Theme::TEXT_PRIMARY
        } else {
            Theme::TEXT_MUTED
        },
        numbers.unwrap_or_else(|| format!("{}", index + 1)),
    );
    ui.add_space(Theme::SPACING_SM);
}

/// "4" or "2–3", by where the spread's pages fall in the reading order.
fn page_numbers(state: &TesseraApp, spread: tessera_document::ids::SpreadId) -> Option<String> {
    let doc = state.active().document();
    let all: Vec<_> = doc.page_ids().collect();
    let pages = doc.pages_of(spread);

    let first = all
        .iter()
        .position(|p| Some(*p) == pages.first().copied())?
        + 1;
    if pages.len() < 2 {
        return Some(first.to_string());
    }
    let last = all.iter().position(|p| Some(*p) == pages.last().copied())? + 1;
    Some(format!("{first}–{last}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{self, Group, Run};

    #[test]
    fn the_panel_starts_closed() {
        assert!(!TesseraApp::headless().pages_window.open);
    }

    #[test]
    fn the_action_opens_and_closes_it() {
        let mut state = TesseraApp::headless();
        actions::run(&mut state, Run::TogglePages);
        assert!(state.pages_window.open);
        actions::run(&mut state, Run::TogglePages);
        assert!(!state.pages_window.open);
    }

    #[test]
    fn the_window_menu_has_exactly_this_one_entry() {
        // The menu bar is generated from the action list, so this is what
        // proves a Window menu appears at all — it was the last of the three
        // milestone 1.5 named as absent for having no commands.
        let named: Vec<&str> = actions::all()
            .iter()
            .filter(|a| a.group == Group::Window)
            .map(|a| a.name)
            .collect();
        assert_eq!(named, vec!["Pages"]);
        assert_eq!(Group::Window.menu(), Some("Window"));
    }

    #[test]
    fn opening_the_panel_is_not_an_edit() {
        let mut state = TesseraApp::headless();
        assert!(!state.active().dirty);
        actions::run(&mut state, Run::TogglePages);
        assert!(!state.active().dirty, "a panel is not a change to the work");
    }

    #[test]
    fn a_lone_page_is_numbered_and_a_facing_pair_is_a_range() {
        let mut state = TesseraApp::headless();
        state.active_mut().document_mut().setup.facing_pages = true;
        apply(&mut state, Command::AddPage);
        apply(&mut state, Command::AddPage);

        let order = state.active().document().spread_order.clone();
        assert_eq!(page_numbers(&state, order[0]).as_deref(), Some("1"));
        assert_eq!(
            page_numbers(&state, order[1]).as_deref(),
            Some("2–3"),
            "the pair reads as a range"
        );
    }

    #[test]
    fn the_numbers_follow_a_reorder() {
        // What a page number *is*: where the page falls in the reading order,
        // not something stored on it.
        let mut state = TesseraApp::headless();
        // One page per spread, so a spread and a page number line up and the
        // reorder is easy to read.
        state.active_mut().document_mut().setup.facing_pages = false;
        apply(&mut state, Command::AddPage);
        apply(&mut state, Command::AddPage);

        let order = state.active().document().spread_order.clone();
        assert_eq!(page_numbers(&state, order[2]).as_deref(), Some("3"));

        apply(&mut state, Command::MoveSpread { from: 2, to: 0 });
        assert_eq!(
            page_numbers(&state, order[2]).as_deref(),
            Some("1"),
            "the same spread is page one now"
        );
    }
}
