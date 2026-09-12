//! The panels as they are actually laid out: sides, splitters and tabs.
//!
//! [`crate::docking`] holds the arrangement; this draws it and lets somebody
//! change it by dragging a tab. The two are separate because the arrangement is
//! the part worth testing and none of it needs a screen.
//!
//! ## A tab bar, not a column of headings
//!
//! The rail used to stack every open panel vertically, each under its own
//! heading. That is fine for two panels and unreadable for six: the one you
//! want is below the fold, and opening another pushes it further down. Tabs put
//! every panel in the stack one click away and cost one row.
//!
//! ## Dragging
//!
//! A tab is a drag source; the tab bar of every stack, and a strip at the far
//! edge of each side, are drop targets. Dropping on a tab bar joins that stack;
//! dropping on an edge strip makes a new stack there, which is how a side gets
//! split. There is nowhere else to drop, and that is deliberate — floating
//! panels are out of scope, so every drop lands somewhere a panel can live.

use egui::{Panel, Ui};

use crate::app::TesseraApp;
use crate::docking::Region;
use crate::theme::Theme;
use crate::view::rail::Dock;

/// How wide a side is before anybody drags its splitter.
pub const WIDTH: f32 = 292.0;

/// The narrowest a side may be dragged.
///
/// Below this the two-column fields in a panel stop fitting and start
/// overlapping their labels. A splitter that can be dragged into an unusable
/// layout is a splitter that will be.
const NARROWEST: f32 = 232.0;

// Checked by the compiler: below this the label and the field in a two-column
// `pair` start overlapping, and a splitter that can be dragged into an unusable
// layout is one that will be.
const _: () = assert!(NARROWEST >= 232.0);

/// How deep the strip at the outer edge of a side is, for dropping a new stack.
const EDGE: f32 = 18.0;

/// What is being dragged: a panel's title.
#[derive(Clone, Debug)]
struct Dragged(String);

/// Draw both sides.
///
/// Left before right so the left splitter is allocated from the window's left
/// edge; egui panels take their room in the order they are declared.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    state.prefs.docking.reconcile();

    for region in Region::ALL {
        side(ui, state, region);
    }

    // A drop that landed nowhere leaves the payload dangling, and the next
    // frame would think a drag was still in progress.
    if ui.input(|i| i.pointer.any_released()) {
        egui::DragAndDrop::clear_payload(ui.ctx());
    }
}

fn side(ui: &mut Ui, state: &mut TesseraApp, region: Region) {
    // A side with nothing open takes no room at all. An empty panel with a
    // splitter is a strip of nothing that still has to be dragged shut.
    let showing: Vec<usize> = (0..state.prefs.docking.stacks(region).len())
        .filter(|at| stack_has_an_open_panel(state, region, *at))
        .collect();
    if showing.is_empty() {
        return;
    }

    let id = match region {
        Region::Left => "dock-left",
        Region::Right => "dock-right",
    };
    let panel = match region {
        Region::Left => Panel::left(id),
        Region::Right => Panel::right(id),
    };

    let showing_panel = panel
        .default_size(WIDTH)
        .min_size(NARROWEST)
        .frame(crate::view::glass::panel_frame(state))
        .show(ui, |ui| {
            crate::view::glass::behind(
                ui,
                state,
                match region {
                    Region::Left => crate::view::glass::Edge::Right,
                    Region::Right => crate::view::glass::Edge::Left,
                },
            );

            // **Below the stacks, not above them.** The strip appears only
            // while something is being dragged, so drawing it first meant it
            // pushed everything down by its own height the moment a drag
            // began — and the pointer that was over a tab found itself over
            // the strip. Releasing dropped the panel into a new stack, which
            // is what a plain click on a tab used to do.
            //
            // Its room is reserved whether it is drawn or not, so the stacks
            // do not resize under the pointer either.
            let room = (ui.available_height() - EDGE - Theme::SPACE_2).max(0.0);
            let each = room / showing.len() as f32;
            for at in showing {
                ui.allocate_ui(egui::vec2(ui.available_width(), each), |ui| {
                    stack(ui, state, region, at);
                });
            }

            edge_target(ui, state, region);
        });

    // The tour points at whichever side is open, and the last one drawn wins if
    // both are. One rect for "the panels": a tour that ringed two things at once
    // would be pointing at nothing in particular.
    state
        .tour
        .mark(crate::tour::Spot::Rail, showing_panel.response.rect);
}

/// Whether anything in this stack is open, and so worth room.
fn stack_has_an_open_panel(state: &TesseraApp, region: Region, at: usize) -> bool {
    state
        .prefs
        .docking
        .stacks(region)
        .get(at)
        .is_some_and(|stack| stack.panels.iter().any(|p| open_by_title(state, p)))
}

fn dock_by_title(title: &str) -> Option<Dock> {
    Dock::ALL.into_iter().find(|d| d.title() == title)
}

fn open_by_title(state: &TesseraApp, title: &str) -> bool {
    dock_by_title(title).is_some_and(|d| d.is_open(state))
}

/// One stack: its tabs, then the panel showing.
fn stack(ui: &mut Ui, state: &mut TesseraApp, region: Region, at: usize) {
    let Some(stack) = state.prefs.docking.stacks(region).get(at).cloned() else {
        return;
    };

    let mut chose = None;
    let mut dropped: Option<(String, usize)> = None;

    let bar = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(Theme::SPACE_1 as i8, 2))
        .fill(Theme::panel_bg_alt())
        .show(ui, |ui| {
            // **A tab bar scrolls; it does not wrap.** Wrapped, egui put the
            // tab that did not fit on a line of its own — and with almost no
            // width left it wrapped *inside the word*, one letter per line, so
            // "Preflight" became nine rows and the bar grew nine rows tall,
            // pushing the panel's own contents down behind it.
            egui::ScrollArea::horizontal()
                .id_salt(("tabs", region, at))
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = Theme::SPACE_1;
                        // Belt as well as braces: a horizontal layout is enough to stop
                        // the row wrapping, and this stops any single label breaking
                        // even if one is given less room than its own text.
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        for (slot, title) in stack.panels.iter().enumerate() {
                            // A shut panel keeps its tab. Closing a panel is not the
                            // same as taking it out of the layout, and a tab that
                            // disappeared would mean reopening it from the Window menu
                            // and finding it somewhere else.
                            let open = open_by_title(state, title);
                            let showing = slot == stack.active && open;

                            let Some(dock) = dock_by_title(title) else {
                                continue;
                            };
                            let response = crate::icons::tab_button(
                                ui,
                                dock.icon(),
                                title,
                                showing,
                                showing,
                                egui::Sense::click_and_drag(),
                            );

                            // **The payload is raised on `drag_started`, not on
                            // press.** `dnd_drag_source` makes a widget a drag
                            // *handle*: it takes the grab cursor on hover and
                            // raises a payload the instant the button goes
                            // down. A tab is a button that can also be dragged,
                            // and the difference was not cosmetic — the payload
                            // brought up the drop strip, the strip pushed the
                            // bar down by its own height, and the pointer that
                            // was over a tab was over the strip instead. A
                            // plain click then dropped the panel into a new
                            // stack. `drag_started` is egui's own answer to
                            // "has this moved far enough to be a drag", so a
                            // click raises no payload at all.
                            if response.drag_started() {
                                egui::DragAndDrop::set_payload(ui.ctx(), Dragged(title.clone()));
                            }

                            if response.clicked() {
                                chose = Some((slot, title.clone()));
                            }
                            if let Some(payload) = response.dnd_release_payload::<Dragged>() {
                                dropped = Some((payload.0.clone(), slot));
                            }
                        }
                    });
                });
        })
        .response;

    // The whole bar takes a drop too, so joining a stack does not mean hitting
    // one of its tabs exactly.
    if let Some(payload) = bar.dnd_release_payload::<Dragged>() {
        dropped = Some((payload.0.clone(), stack.panels.len()));
    }

    if let Some((slot, title)) = chose {
        // Clicking a tab shows that panel *and* opens it: a tab you click and
        // nothing happens to is a tab that looks broken, and the reason would
        // be a checkbox in a menu somewhere else.
        if let Some(dock) = dock_by_title(&title) {
            dock.set_open(state, true);
        }
        if let Some(stack) = stack_mut(state, region, at) {
            stack.active = slot;
        }
    }
    if let Some((title, at_slot)) = dropped {
        state.prefs.docking.place(&title, region, at, at_slot);
        if let Some(dock) = dock_by_title(&title) {
            dock.set_open(state, true);
        }
    }

    ui.painter().hline(
        bar.rect.x_range(),
        bar.rect.bottom(),
        egui::Stroke::new(1.0, Theme::rule()),
    );

    // The panel itself, which may be none: every tab in this stack can be shut.
    let Some(title) = state
        .prefs
        .docking
        .stacks(region)
        .get(at)
        .and_then(|stack| stack.showing())
        .map(str::to_string)
    else {
        return;
    };
    if !open_by_title(state, &title) {
        ui.add_space(Theme::SPACE_2);
        ui.colored_label(Theme::text_muted(), format!("{title} is closed."));
        return;
    }
    let Some(dock) = dock_by_title(&title) else {
        return;
    };

    egui::ScrollArea::vertical()
        .id_salt(("dock", region, at))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = Theme::SPACE_1;
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(
                        Theme::SPACE_2 as i8,
                        Theme::SPACE_2 as i8,
                    ))
                    .show(ui, |ui| crate::view::rail::body(ui, state, dock));
            });
        });
}

fn stack_mut(
    state: &mut TesseraApp,
    region: Region,
    at: usize,
) -> Option<&mut crate::docking::Stack> {
    match region {
        Region::Left => state.prefs.docking.left.get_mut(at),
        Region::Right => state.prefs.docking.right.get_mut(at),
    }
}

/// A thin strip along the outer edge that makes a new stack when dropped on.
///
/// Drawn only while something is being dragged. A permanent strip of dead space
/// down the side of every panel is eighteen points of window nobody asked for.
fn edge_target(ui: &mut Ui, state: &mut TesseraApp, region: Region) {
    if !egui::DragAndDrop::has_payload_of_type::<Dragged>(ui.ctx()) {
        return;
    }

    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), EDGE), egui::Sense::hover());
    let hovered = response.contains_pointer();
    ui.painter().rect_filled(
        rect,
        Theme::RADIUS,
        if hovered {
            Theme::accent()
        } else {
            Theme::hover_bg()
        },
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "New group",
        egui::TextStyle::Small.resolve(ui.style()),
        Theme::text_primary(),
    );

    if let Some(payload) = response.dnd_release_payload::<Dragged>() {
        let end = state.prefs.docking.stacks(region).len();
        state.prefs.docking.place(&payload.0, region, end, 0);
        if let Some(dock) = dock_by_title(&payload.0) {
            dock.set_open(state, true);
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn every_dock_has_an_icon_to_be_a_tab_with() {
        // Inactive tabs are drawn as their icon and nothing else. A dock whose
        // icon did not resolve would be a blank clickable square, which is
        // worse than a name that does not fit.
        for dock in Dock::ALL {
            let outlines = dock.icon().paths();
            assert!(
                !outlines.is_empty(),
                "{} has no icon to be a tab with",
                dock.title()
            );
        }
    }

    #[test]
    fn the_titles_do_not_fit_a_rail_spelled_out() {
        // The reason only the showing tab is named. If this ever stops being
        // true the icons could go, and this says so rather than leaving the
        // decision as a comment nobody rechecks.
        //
        // **The padding is most of it**, which is what the first version of
        // this test got wrong: the words alone come to about 264 points and
        // would fit, but every tab is a button with padding on both sides and
        // a gap to the next one, and that is another 130.
        let words: f32 = Dock::ALL.iter().map(|d| d.title().len() as f32 * 6.0).sum();
        let padding = Dock::ALL.len() as f32 * Theme::SPACE_2 * 2.0;
        let gaps = (Dock::ALL.len() - 1) as f32 * Theme::SPACE_2;
        let needed = words + padding + gaps;

        assert!(
            needed > WIDTH,
            "the titles now fit in {WIDTH} points ({needed} needed): naming \
             every tab is back on the table"
        );
    }
    use super::*;

    #[test]
    fn every_panel_title_names_a_dock() {
        // The layout stores titles and the view turns them back into panels. A
        // title that resolves to nothing is a tab that draws an empty area, and
        // `Docking::reconcile` can only drop what it can recognise.
        for dock in Dock::ALL {
            assert_eq!(dock_by_title(dock.title()), Some(dock));
        }
    }

    #[test]
    fn a_side_with_nothing_open_takes_no_room() {
        // An empty panel with a splitter is a strip of nothing that still has to
        // be dragged shut.
        let mut state = TesseraApp::headless();
        for dock in Dock::ALL {
            dock.set_open(&mut state, false);
        }
        state.prefs.docking.reconcile();
        for region in Region::ALL {
            for at in 0..state.prefs.docking.stacks(region).len() {
                assert!(!stack_has_an_open_panel(&state, region, at));
            }
        }
    }

    #[test]
    fn a_stack_counts_as_showing_when_any_one_of_its_panels_is_open() {
        let mut state = TesseraApp::headless();
        for dock in Dock::ALL {
            dock.set_open(&mut state, false);
        }
        Dock::Pages.set_open(&mut state, true);
        state.prefs.docking.reconcile();

        let (region, at, _) = state.prefs.docking.find("Pages").expect("placed");
        assert!(stack_has_an_open_panel(&state, region, at));
    }
}
