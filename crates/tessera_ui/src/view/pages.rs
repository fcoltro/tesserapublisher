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

use tessera_document::ids::{MasterId, PageId, SpreadId};

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

/// The tallest the page list grows before it scrolls inside itself.
const LIST: f32 = 260.0;

/// The section, as it sits in the rail.
///
/// The buttons come last but take a fixed height of their own, so the list
/// above them can grow without the strip growing with it — the waste the
/// floating panel was reported for.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    masters(ui, state);

    // The list scrolls inside a bounded height rather than growing without
    // limit. Left to grow, twenty pages push the buttons off the bottom of the
    // rail and every panel below this one with them.
    egui::ScrollArea::vertical()
        .id_salt("pages-list")
        .max_height(LIST)
        .auto_shrink([false, true])
        .show(ui, |ui| body(ui, state));

    ui.add_space(Theme::SPACE_2);
    actions(ui, state);
}

/// The parent pages, listed above the document's own.
///
/// InDesign's arrangement: parents are their own short list at the top of the
/// panel, with the document's pages under them. A parent is **edited in
/// isolation** — double-clicking one opens it on its own canvas — rather than
/// sitting in the scroll a person is trying to lay out in.
fn masters(ui: &mut Ui, state: &mut TesseraApp) {
    let masters = state.active().document().master_order.clone();

    // The heading carries the add button, so "add a parent" reads as part of
    // the parent list rather than as a fifth unexplained glyph in the strip at
    // the foot, which is where it was and what it looked like.
    ui.horizontal(|ui| {
        crate::view::panels::group_label_pub(ui, "Parent pages");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::view::panels::icon_button(
                ui,
                crate::icons::Icon::Plus,
                "Add parent page",
                false,
            ) {
                apply(state, Command::AddMaster);
            }
        });
    });

    let current = crate::view::panels::current_page(state);
    let applied = current.and_then(|p| state.active().document().pages[p].master);
    let editing = state.editing_master;

    let mut chosen: Option<MasterId> = None;
    let mut open: Option<MasterId> = None;
    let mut detach = false;

    // "None" first, which is how a page is taken off a parent. InDesign has
    // the same entry for the same reason: without it, the only way to say "no
    // parent" is to guess that clicking the current one twice does it.
    {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), Theme::ROW),
            egui::Sense::click(),
        );
        let painter = ui.painter_at(rect);
        if applied.is_none() {
            painter.rect_filled(rect, Theme::RADIUS, Theme::selected_bg());
        } else if response.hovered() {
            painter.rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
        }
        painter.text(
            egui::pos2(rect.left() + Theme::SPACE_2, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "None",
            egui::TextStyle::Body.resolve(ui.style()),
            Theme::text_muted(),
        );
        if response
            .on_hover_text("Build this page on no parent")
            .clicked()
        {
            detach = true;
        }
    }

    for id in masters {
        let Some(master) = state.active().document().masters.get(id).cloned() else {
            continue;
        };
        let pages = state.active().document().pages_of_master(id);
        let holds = pages
            .iter()
            .map(|p| state.active().document().frames_on_page(*p).len())
            .sum::<usize>();
        let on_this_page = pages.iter().any(|p| Some(*p) == applied);

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), Theme::ROW),
            egui::Sense::click(),
        );
        let painter = ui.painter_at(rect);
        if editing == Some(id) {
            // Being edited beats being applied: it is where you are, not what
            // this page happens to use.
            painter.rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
            painter.rect_stroke(
                rect,
                Theme::RADIUS,
                egui::Stroke::new(1.0, Theme::accent()),
                egui::StrokeKind::Inside,
            );
        } else if on_this_page {
            painter.rect_filled(rect, Theme::RADIUS, Theme::selected_bg());
        } else if response.hovered() {
            painter.rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
        }
        painter.text(
            egui::pos2(rect.left() + Theme::SPACE_2, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &master.name,
            egui::TextStyle::Body.resolve(ui.style()),
            Theme::text_primary(),
        );
        painter.text(
            egui::pos2(rect.right() - Theme::SPACE_2, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            if holds == 1 {
                "1 item".to_string()
            } else {
                format!("{holds} items")
            },
            egui::TextStyle::Small.resolve(ui.style()),
            Theme::text_muted(),
        );

        let response = response
            .on_hover_text("Click to build this page on it. Double-click to open and edit it.");
        if response.double_clicked() {
            open = Some(id);
        } else if response.clicked() {
            chosen = Some(id);
        }
    }

    if let Some(page) = current {
        if detach {
            apply(state, Command::ApplyMaster { page, master: None });
        } else if let Some(master) = chosen {
            apply(
                state,
                Command::ApplyMaster {
                    page,
                    master: Some(master),
                },
            );
        }
    }
    if let Some(master) = open {
        // Toggling: double-clicking the parent already open closes it, so the
        // way in is the way out as well as the bar at the top of the canvas.
        let now = if state.editing_master == Some(master) {
            None
        } else {
            Some(master)
        };
        state.edit_master(now);
    }

    ui.add_space(Theme::SPACE_3);
    crate::view::panels::group_label_pub(ui, "Pages");
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    let spreads = state.active().document().spread_order.clone();
    let current = state
        .active()
        .current_spread
        .min(spreads.len().saturating_sub(1));

    let facing = state.active().document().setup.facing_pages;
    let columns = if facing { 2.0 } else { 1.0 };
    let slot = egui::vec2(PAGE * columns + FOLD, PAGE * 1.3);

    // Decided while drawing, acted on afterwards: moving a page mid-walk would
    // renumber what is still being drawn.
    let mut turn_to: Option<usize> = None;
    let mut dragging: Option<PageId> = None;
    let mut dropped = false;

    // Every page slot in reading order, which is what a drop position counts.
    let mut slots: Vec<egui::Rect> = Vec::new();

    for (index, spread) in spreads.iter().enumerate() {
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(slot.x, slot.y + LABEL), egui::Sense::hover());
        let sheet = egui::Rect::from_min_size(rect.min, slot);

        for (column, page) in state
            .active()
            .document()
            .pages_of(*spread)
            .iter()
            .enumerate()
        {
            let side = column_of(state, *spread, column, facing);
            let at = egui::Rect::from_min_size(
                sheet.min + egui::vec2(side * (PAGE + FOLD), 0.0),
                egui::vec2(PAGE, slot.y),
            );
            slots.push(at);

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

        // The page numbers this spread holds, under it.
        let numbers = page_numbers(state, *spread).unwrap_or_else(|| format!("{}", index + 1));
        ui.painter().text(
            egui::pos2(sheet.center().x, sheet.bottom() + LABEL / 2.0),
            egui::Align2::CENTER_CENTER,
            numbers,
            egui::TextStyle::Small.resolve(ui.style()),
            if index == current {
                Theme::text_primary()
            } else {
                Theme::text_muted()
            },
        );
    }

    let landing = dragging
        .and(ui.ctx().pointer_interact_pos())
        .map(|p| landing(p, &slots));

    // The line saying where it would land. Without one, a drag is a gesture
    // with no target and the page simply appears somewhere afterwards.
    if let Some(at) = landing
        && let Some(marker) = marker(at, &slots)
    {
        ui.painter().rect_filled(marker, 1.0, Theme::accent());
    }

    if let Some(at) = turn_to {
        state.active_mut().current_spread = at;
        state.active_mut().fitted = false;
    }
    if dropped
        && let Some(id) = dragging
        && let Some(to) = landing
    {
        apply(state, Command::MovePage { id, to });
    }
}

/// Which place in the reading order a drop at `p` means.
///
/// Counted rather than hit-tested: a page goes *after* every slot the pointer
/// is past, where past means a row below, or the same row and beyond the
/// middle of the page. Dropping onto the right half of page four means five.
fn landing(p: egui::Pos2, slots: &[egui::Rect]) -> usize {
    slots
        .iter()
        .filter(|r| p.y > r.bottom() || (p.y >= r.top() && p.x > r.center().x))
        .count()
}

/// Where to draw the line for a drop at `at`.
///
/// On the leading edge of the slot it would take, or the trailing edge of the
/// last one when it goes at the end.
fn marker(at: usize, slots: &[egui::Rect]) -> Option<egui::Rect> {
    let (slot, edge) = match slots.get(at) {
        Some(slot) => (slot, slot.left()),
        None => {
            let slot = slots.last()?;
            (slot, slot.right())
        }
    };
    Some(egui::Rect::from_min_size(
        egui::pos2(edge - MARKER / 2.0, slot.top()),
        egui::vec2(MARKER, slot.height()),
    ))
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
        // A thumbnail block is a few pixels of one colour. Drawing the whole
        // ramp at that size would cost a gradient per object for something
        // nobody can see, so it takes one colour from it.
        let [r, g, bl, a] = frame.fill.representative().to_rgb_f32();
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
                Theme::accent()
            } else {
                Theme::border()
            },
        ),
        egui::StrokeKind::Inside,
    );
}

/// Add, duplicate and delete: a strip of fixed height at the foot.
fn actions(ui: &mut Ui, state: &mut TesseraApp) {
    // `horizontal`, not `horizontal_centered`. The centred version allocates
    // **all** the height it is offered and centres its contents in it, which
    // is what left a void the size of the rail under these four buttons and
    // pushed Layers and Styles to the bottom of the panel.
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
        assert_eq!(named, vec!["Pages", "Layers", "Swatches"]);
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

    // --- where a drop lands -------------------------------------------------

    fn slot(x: f32, y: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(46.0, 60.0))
    }

    /// Two spreads: page one alone on the right, then two facing.
    fn a_short_document() -> Vec<egui::Rect> {
        vec![slot(48.0, 0.0), slot(0.0, 76.0), slot(48.0, 76.0)]
    }

    #[test]
    fn dropping_on_the_left_half_of_a_page_goes_before_it() {
        let slots = a_short_document();
        assert_eq!(landing(egui::pos2(52.0, 30.0), &slots), 0);
    }

    #[test]
    fn dropping_on_the_right_half_of_a_page_goes_after_it() {
        let slots = a_short_document();
        assert_eq!(landing(egui::pos2(90.0, 30.0), &slots), 1);
    }

    #[test]
    fn dropping_on_a_later_row_counts_every_page_above_it() {
        let slots = a_short_document();
        assert_eq!(
            landing(egui::pos2(4.0, 100.0), &slots),
            1,
            "before the second row's first page"
        );
        assert_eq!(
            landing(egui::pos2(90.0, 100.0), &slots),
            3,
            "past both of them"
        );
    }

    #[test]
    fn dropping_below_everything_goes_last() {
        let slots = a_short_document();
        assert_eq!(landing(egui::pos2(20.0, 500.0), &slots), 3);
    }

    #[test]
    fn the_marker_sits_on_the_leading_edge_of_the_slot_taken() {
        let slots = a_short_document();
        let at = marker(1, &slots).expect("a marker");
        assert!((at.center().x - slots[1].left()).abs() < 0.01);
    }

    #[test]
    fn a_drop_at_the_end_marks_the_trailing_edge_of_the_last_slot() {
        let slots = a_short_document();
        let at = marker(3, &slots).expect("a marker");
        assert!((at.center().x - slots[2].right()).abs() < 0.01);
    }

    #[test]
    fn a_marker_with_nowhere_to_go_is_no_marker() {
        assert!(marker(0, &[]).is_none());
    }

    // --- moving a page ------------------------------------------------------

    #[test]
    fn moving_a_page_is_undoable() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddPage);
        apply(&mut state, Command::AddPage);
        let before: Vec<_> = state.active().document().page_ids().collect();

        apply(
            &mut state,
            Command::MovePage {
                id: before[0],
                to: 2,
            },
        );
        let after: Vec<_> = state.active().document().page_ids().collect();
        assert_ne!(after, before);

        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().page_ids().collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn a_moved_page_never_leaves_a_spread_of_two_starting_on_a_recto() {
        // The shape the panel could not recover from: both pages drawn in the
        // right-hand column, and an empty left column that could not be
        // dropped onto.
        let mut state = TesseraApp::headless();
        for _ in 0..4 {
            apply(&mut state, Command::AddPage);
        }
        let last = state.active().document().page_ids().last().expect("a page");

        apply(&mut state, Command::MovePage { id: last, to: 0 });

        let doc = state.active().document();
        let width = doc.first_page_bounds().width;
        for page in doc.page_ids() {
            let column = (doc.pages[page].bounds.x / width).round() as i32;
            assert!((0..=1).contains(&column), "page off the sheet at {column}");
        }
    }
}
