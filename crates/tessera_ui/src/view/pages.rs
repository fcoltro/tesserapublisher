//! The pages panel: which spreads exist, which one you are on, and reordering.
//!
//! Thumbnails are **schematic**, drawn with egui's painter from the document:
//! each frame becomes a filled rectangle in its own colour. A real thumbnail
//! would mean rendering every spread to a texture and keeping those textures in
//! step with the document, which is a cache with an invalidation rule — and at
//! the size these are drawn, the schematic says the same thing. It also costs
//! no GPU and cannot fall behind, because it is redrawn from the document the
//! canvas is drawn from.
//!
//! **A page is drawn as a page, not a spread as a rectangle.** Every spread
//! occupies the same slot — two page-widths across when pages face — and each
//! page is painted in its own column with a gap at the fold. That makes three
//! things legible that were not: how many pages a spread holds, which side of
//! the fold each one is on, and that page one is a recto sitting to the right
//! of the spine with nothing facing it.

use egui::Ui;

use tessera_document::ids::{PageId, SpreadId};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// How wide one page is drawn, in screen points.
const PAGE: f32 = 46.0;

/// The gap at the fold, so two facing pages read as two sheets.
const FOLD: f32 = 2.0;

/// How much room the label under a spread takes.
const LABEL: f32 = 16.0;

/// How thick the line marking where a dragged page would land is.
const MARKER: f32 = 2.0;

/// The window, if it is open.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    if !state.pages_window.open {
        return;
    }

    let mut open = true;
    egui::Window::new("Pages")
        .open(&mut open)
        .default_width(180.0)
        .default_height(420.0)
        .show(ui.ctx(), |ui| {
            // The buttons are a fixed strip at the foot, laid out **before**
            // the list so they keep their height when the panel grows. Put
            // after it, they were pushed down by however much empty room the
            // list had, which is the waste the panel was reported for.
            egui::Panel::bottom("pages-actions")
                .exact_size(30.0)
                .resizable(false)
                .show(ui, |ui| actions(ui, state));

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| body(ui, state));
        });
    state.pages_window.open = open;
}

/// Where a dragged page would go if it were dropped now.
#[derive(Clone, Copy, PartialEq)]
struct Landing {
    spread: SpreadId,
    at: usize,
    /// Where to draw the line saying so.
    marker: egui::Rect,
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    let doc_spreads = state.active().document().spread_order.clone();
    let current = state
        .active()
        .current_spread
        .min(doc_spreads.len().saturating_sub(1));

    let facing = state.active().document().setup.facing_pages;
    let columns = if facing { 2.0 } else { 1.0 };
    let slot = egui::vec2(PAGE * columns + FOLD, PAGE * 1.3);

    // Decided while drawing, acted on afterwards: moving a page mid-walk would
    // renumber what is still being drawn.
    let mut turn_to: Option<usize> = None;
    let mut dragging: Option<PageId> = None;
    let mut landing: Option<Landing> = None;
    let mut dropped = false;

    let pointer = ui.ctx().pointer_interact_pos();

    for (index, spread) in doc_spreads.iter().enumerate() {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(slot.x, slot.y + LABEL), egui::Sense::hover());
        let sheet = egui::Rect::from_min_size(rect.min, slot);

        let pages = state.active().document().pages_of(*spread);
        for (column, page) in pages.iter().enumerate() {
            let side = column_of(state, *spread, column, facing);
            let at = egui::Rect::from_min_size(
                sheet.min + egui::vec2(side * (PAGE + FOLD), 0.0),
                egui::vec2(PAGE, slot.y),
            );

            let response = ui.interact(
                at,
                egui::Id::new(("page", *page)),
                egui::Sense::click_and_drag(),
            );

            thumbnail(ui, state, *page, at, index == current);

            if response.dragged() {
                dragging = Some(*page);
            }
            if response.drag_stopped() {
                dropped = true;
                dragging = Some(*page);
            }
            if response.clicked() {
                turn_to = Some(index);
            }
        }

        // Where a drop would land, worked out from the pointer rather than
        // from a drop zone: the slots are a known grid, so this is arithmetic,
        // and arithmetic cannot disagree with what was painted.
        if let Some(p) = pointer
            && dragging.is_some()
            && sheet.expand2(egui::vec2(0.0, LABEL / 2.0)).contains(p)
        {
            let at = ((p.x - sheet.left()) / (PAGE + FOLD)).round().max(0.0) as usize;
            let at = at.min(pages.len());
            landing = Some(Landing {
                spread: *spread,
                at,
                marker: egui::Rect::from_min_size(
                    egui::pos2(
                        sheet.left() + at as f32 * (PAGE + FOLD) - MARKER / 2.0,
                        sheet.top(),
                    ),
                    egui::vec2(MARKER, slot.y),
                ),
            });
        }

        // The page numbers this spread holds, under it.
        let numbers = page_numbers(state, *spread).unwrap_or_else(|| format!("{}", index + 1));
        ui.painter().text(
            egui::pos2(sheet.center().x, sheet.bottom() + LABEL / 2.0),
            egui::Align2::CENTER_CENTER,
            numbers,
            egui::TextStyle::Small.resolve(ui.style()),
            if index == current {
                Theme::TEXT_PRIMARY
            } else {
                Theme::TEXT_MUTED
            },
        );
    }

    // The line saying where it would land. Without one, a drag is a gesture
    // with no target and the page simply appears somewhere afterwards.
    if let Some(landing) = landing
        && dragging.is_some()
    {
        ui.painter().rect_filled(landing.marker, 1.0, Theme::ACCENT);
    }

    if let Some(at) = turn_to {
        state.active_mut().current_spread = at;
        state.active_mut().fitted = false;
    }
    if dropped
        && let Some(id) = dragging
        && let Some(landing) = landing
    {
        apply(
            state,
            Command::MovePage {
                id,
                to: landing.spread,
                at: landing.at,
            },
        );
    }
}

/// Which column of the slot this page is drawn in.
///
/// A spread of one page is not a spread of two with a hole in it: page one is
/// a recto and belongs on the right of the fold, a final lone page is a verso
/// and belongs on the left. The document already positions them that way, so
/// this reads the answer off the geometry rather than deciding it again — two
/// places deciding the same thing is two places to disagree.
fn column_of(state: &TesseraApp, spread: SpreadId, column: usize, facing: bool) -> f32 {
    if !facing {
        return 0.0;
    }
    let doc = state.active().document();
    let pages = doc.pages_of(spread);
    let Some(first) = pages.first().and_then(|p| doc.pages.get(*p)) else {
        return column as f32;
    };
    // `x` is a whole number of page widths from the spread's left edge.
    let offset = (first.bounds.x / first.bounds.width.max(1.0)).round() as f32;
    offset + column as f32
}

/// One page, drawn as its contents blocked in.
fn thumbnail(ui: &Ui, state: &TesseraApp, page: PageId, at: egui::Rect, current: bool) {
    let doc = state.active().document();
    let Some(bounds) = doc.pages.get(page).map(|p| p.bounds) else {
        return;
    };
    let scale = f64::from(at.width()) / bounds.width.max(1.0);

    let painter = ui.painter_at(at);
    painter.rect_filled(at, 1.0, egui::Color32::WHITE);

    // What stands on the page, in paint order. Layers span the document, so
    // this asks the page what is on it rather than walking layers it owns.
    for frame in doc.frames_on_page(page) {
        let visible = doc
            .layer_of_frame(frame)
            .and_then(|l| doc.layers.get(l))
            .is_some_and(|l| l.visible);
        if !visible {
            continue;
        }
        let Some(frame) = doc.frame(frame) else {
            continue;
        };
        let b = frame.bounds;
        let block = egui::Rect::from_min_size(
            at.min
                + egui::vec2(
                    ((b.x - bounds.x) * scale) as f32,
                    ((b.y - bounds.y) * scale) as f32,
                ),
            egui::vec2((b.width * scale) as f32, (b.height * scale) as f32),
        );
        let [r, g, bl, a] = frame.fill.to_rgb_f32();
        painter.rect_filled(
            block.intersect(at),
            0.0,
            egui::Color32::from_rgba_unmultiplied(
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (bl * 255.0) as u8,
                // Never fully transparent: a text frame's fill is clear by
                // default, and a thumbnail showing nothing where something is
                // would be a lie.
                ((a * 255.0) as u8).max(60),
            ),
        );
    }

    painter.rect_stroke(
        at,
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
}

/// Add, duplicate and delete: a strip of fixed height at the foot.
fn actions(ui: &mut Ui, state: &mut TesseraApp) {
    ui.horizontal_centered(|ui| {
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

/// "4" or "2–3", by where the spread's pages fall in the reading order.
fn page_numbers(state: &TesseraApp, spread: SpreadId) -> Option<String> {
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
    fn the_window_menu_lists_the_panels_there_are() {
        // The menu bar is generated from the action list, so this is what
        // proves a Window menu appears at all — it was the last of the three
        // milestone 1.5 named as absent for having no commands. Exact, so a
        // panel cannot be added to the menu without being added here: an entry
        // for an unbuilt panel is the lie the previous codebase told often.
        let named: Vec<&str> = actions::all()
            .iter()
            .filter(|a| a.group == Group::Window)
            .map(|a| a.name)
            .collect();
        assert_eq!(named, vec!["Pages", "Layers"]);
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

    // --- which column a page is drawn in ------------------------------------

    #[test]
    fn page_one_is_drawn_to_the_right_of_the_fold() {
        // Reported from real use: a lone first page drew in the left column,
        // which reads as the back of a sheet and puts the whole document a
        // page out of step.
        let state = TesseraApp::headless();
        let spread = state.active().document().spread_order[0];

        assert_eq!(column_of(&state, spread, 0, true), 1.0);
    }

    #[test]
    fn a_facing_pair_is_drawn_either_side_of_the_fold() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddPage);
        apply(&mut state, Command::AddPage);
        let spread = state.active().document().spread_order[1];

        assert_eq!(column_of(&state, spread, 0, true), 0.0, "the verso");
        assert_eq!(column_of(&state, spread, 1, true), 1.0, "and the recto");
    }

    #[test]
    fn a_final_lone_page_is_drawn_to_the_left() {
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            apply(&mut state, Command::AddPage);
        }
        let last = *state
            .active()
            .document()
            .spread_order
            .last()
            .expect("a spread");

        assert_eq!(column_of(&state, last, 0, true), 0.0);
    }

    #[test]
    fn pages_that_do_not_face_are_all_drawn_in_one_column() {
        let mut state = TesseraApp::headless();
        state.active_mut().document_mut().setup.facing_pages = false;
        apply(&mut state, Command::AddPage);
        let spread = state.active().document().spread_order[1];

        assert_eq!(
            column_of(&state, spread, 0, false),
            0.0,
            "with no spine there is no side to be on"
        );
    }

    // --- moving a page ------------------------------------------------------

    #[test]
    fn a_page_can_be_moved_within_its_spread() {
        // What turning a spread round means, and what the panel could not do.
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddPage);
        apply(&mut state, Command::AddPage);
        let spread = state.active().document().spread_order[1];
        let pages = state.active().document().pages_of(spread);

        apply(
            &mut state,
            Command::MovePage {
                id: pages[1],
                to: spread,
                at: 0,
            },
        );

        assert_eq!(
            state.active().document().pages_of(spread),
            vec![pages[1], pages[0]]
        );
    }

    #[test]
    fn moving_a_page_is_undoable() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddPage);
        apply(&mut state, Command::AddPage);
        let spread = state.active().document().spread_order[1];
        let before = state.active().document().pages_of(spread);

        apply(
            &mut state,
            Command::MovePage {
                id: before[1],
                to: spread,
                at: 0,
            },
        );
        apply(&mut state, Command::Undo);

        assert_eq!(state.active().document().pages_of(spread), before);
    }
}
