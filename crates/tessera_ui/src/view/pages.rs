//! The pages panel: the parent pages, the document's pages, which of them
//! are chosen, and what can be done to them.
//!
//! **A page is drawn as a page, not a spread as a rectangle.** Facing pages
//! sit either side of one spine running down the list, as they sit either
//! side of the fold of the book, so every recto lines up with every other
//! and page one stands alone on the right. Pages that do not face are laid
//! out in rows across the panel instead of one to a row down its left.
//!
//! Each page is the page itself, drawn by the canvas's renderer, with what a
//! page is built from marked on it: the letter of its parent in its corner,
//! and a mark over it where a numbering section starts.
//!
//! **The panel chooses pages, and its actions act on the chosen ones.** They
//! acted on "the current page" — the first page of the spread the canvas was
//! turned to — so a parent could not be put on a right-hand page from here
//! at all, and nothing could be done to three pages at once. A click chooses
//! a page and turns to it; Shift-click chooses a run, Ctrl-click one more.
//! With none chosen, the page being worked on is what they act on.

use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};

use tessera_document::ids::{MasterId, PageId, SpreadId};
use tessera_layout::resolve::Scope;

use crate::app::{TesseraApp, ThumbnailSize};
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// The gap at the fold, so two facing pages read as two sheets.
const FOLD: f32 = 2.0;

/// The room under a page for its number.
const LABEL: f32 = 20.0;

/// The room over a page for the mark saying a section starts there.
const MARK: f32 = 9.0;

/// Between one row of pages and the next.
const ROW_GAP: f32 = 4.0;

/// Between pages that do not face, side by side in a row.
const GRID_GAP: f32 = 14.0;

/// Kept clear at either side of the list.
const PAD: f32 = 6.0;

/// Where [`crate::view::show`] leaves the GPU for the thumbnails.
pub(crate) const GPU: &str = "tessera-gpu";

/// How many thumbnails may be rendered in one frame. The rest keep the one
/// they had and are drawn again on the next, so a keystroke in a hundred-page
/// document redraws two pages' previews, not a hundred.
const RENDERS_PER_FRAME: u8 = 2;

/// How thick the line marking where a dragged page would land is.
const MARKER: f32 = 2.0;

/// The least the page list is given, however little room the rail has.
const MIN_LIST: f32 = 140.0;

/// The height the footer's actions take under the list.
const FOOTER: f32 = 44.0;

/// A parent's row, and how wide one of its pages is drawn in it.
const PARENT_ROW: f32 = 30.0;
const PARENT_PAGE: f32 = 15.0;

/// A parent being dragged onto a page: `None` is [None], which takes the
/// parent off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ParentPayload(Option<MasterId>);

/// The section, as it sits in the rail.
///
/// The list runs down to the foot of the rail and the footer sits under it,
/// where InDesign's does. The list was capped at 260 points and scrolled
/// inside itself, which showed three spreads of a book with the rest of the
/// rail empty below them.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    tidy(state);
    parents(ui, state);
    ui.add_space(Theme::space_3());
    heading(ui, state);

    // The rail scrolls its panel, so the room left is what is left *visible*:
    // the clip, not the unbounded height a scrolling area offers — less what
    // the foot of the panel took last frame, which grows while the Insert
    // pages form is open.
    let foot_id = egui::Id::new("pages-foot-height");
    let foot = ui.data(|d| d.get_temp::<f32>(foot_id)).unwrap_or(FOOTER);
    let room = (ui.clip_rect().bottom() - ui.cursor().top() - foot).max(MIN_LIST);
    egui::ScrollArea::vertical()
        .id_salt("pages-list")
        .max_height(room)
        .min_scrolled_height(room)
        .auto_shrink([false, false])
        .show(ui, |ui| body(ui, state));
    let top = ui.cursor().top();
    if state.pages_window.inserting.is_some() {
        insert_form(ui, state);
    }
    footer(ui, state);
    let measured = (ui.cursor().top() - top).max(FOOTER);
    if ui.data(|d| d.get_temp::<f32>(foot_id)) != Some(measured) {
        ui.data_mut(|d| d.insert_temp(foot_id, measured));
        ui.ctx().request_repaint();
    }
}

/// Where Insert pages puts the new pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InsertAt {
    /// After the last page chosen, or the page being worked on.
    #[default]
    AfterChosen,
    Start,
    End,
}

/// What the new pages are built on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InsertParent {
    /// The parent of the page each follows, as one page inserted takes.
    #[default]
    AsBefore,
    None,
    Master(MasterId),
}

/// InDesign's Insert Pages: how many, where, and on what parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertForm {
    pub count: usize,
    pub at: InsertAt,
    pub parent: InsertParent,
}

impl Default for InsertForm {
    fn default() -> Self {
        Self {
            count: 1,
            at: InsertAt::default(),
            parent: InsertParent::default(),
        }
    }
}

/// The most pages one Insert makes: a book's worth, and no more — a slip
/// of the finger on the count should not be a document of ten thousand.
const MOST_INSERTED: usize = 500;

/// Insert pages, as a card over the foot of the panel.
fn insert_form(ui: &mut Ui, state: &mut TesseraApp) {
    let Some(mut form) = state.pages_window.inserting else {
        return;
    };
    let chosen = summary(state);
    let doc = state.active().document();
    let masters: Vec<(MasterId, String)> = doc
        .master_ids()
        .filter_map(|id| doc.masters.get(id).map(|m| (id, m.name.clone())))
        .collect();
    let mut go = false;
    let mut cancel = false;
    super::style_ui::card(ui, Some("Insert pages"), |ui| {
        crate::view::panels::field(ui, "Pages", |ui| {
            crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut form.count)
                        .range(1..=MOST_INSERTED)
                        .speed(0.2),
                ),
                "How many pages",
            );
        });
        crate::view::panels::field(ui, "Where", |ui| {
            let said = |at: InsertAt| match at {
                InsertAt::AfterChosen => format!("After {}", chosen.to_lowercase()),
                InsertAt::Start => "At the start".to_string(),
                InsertAt::End => "At the end".to_string(),
            };
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("insert-where")
                    .selected_text(said(form.at))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for at in [InsertAt::AfterChosen, InsertAt::Start, InsertAt::End] {
                            ui.selectable_value(&mut form.at, at, said(at));
                        }
                    })
                    .response,
                "Where",
                egui::WidgetType::ComboBox,
                None,
            );
        });
        crate::view::panels::field(ui, "Parent", |ui| {
            let said = |parent: InsertParent| match parent {
                InsertParent::AsBefore => "As the page before".to_string(),
                InsertParent::None => "[None]".to_string(),
                InsertParent::Master(id) => masters
                    .iter()
                    .find(|(m, _)| *m == id)
                    .map_or_else(|| "A parent".to_string(), |(_, name)| name.clone()),
            };
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("insert-parent")
                    .selected_text(said(form.parent))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut form.parent,
                            InsertParent::AsBefore,
                            said(InsertParent::AsBefore),
                        );
                        ui.selectable_value(&mut form.parent, InsertParent::None, "[None]");
                        for (id, name) in &masters {
                            ui.selectable_value(
                                &mut form.parent,
                                InsertParent::Master(*id),
                                name.as_str(),
                            );
                        }
                    })
                    .response,
                "Parent",
                egui::WidgetType::ComboBox,
                None,
            );
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let label = if form.count == 1 {
                "Insert 1 page".to_string()
            } else {
                format!("Insert {} pages", form.count)
            };
            if super::panel_ui::action(ui, Icon::Plus, &label).clicked() {
                go = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    state.pages_window.inserting = Some(form);
    if go {
        run(state, PageAct::InsertMany(form));
        state.pages_window.inserting = None;
    } else if cancel {
        state.pages_window.inserting = None;
    }
}

/// Forget chosen pages that have gone — deleted, undone away, or of another
/// document — so no action is sent to a page that is not there.
fn tidy(state: &mut TesseraApp) {
    let key = state.active;
    let doc = state.active().document();
    let pages: Vec<PageId> = doc.page_ids().collect();
    let renamed_gone = state
        .pages_window
        .renaming
        .as_ref()
        .is_some_and(|(master, _)| !doc.masters.contains_key(*master));
    let window = &mut state.pages_window;
    if window.of != Some(key) {
        window.selected.clear();
        window.anchor = None;
        window.of = Some(key);
    }
    window.selected.retain(|page| pages.contains(page));
    if window.anchor.is_some_and(|anchor| !pages.contains(&anchor)) {
        window.anchor = None;
    }
    if renamed_gone {
        window.renaming = None;
    }
}

/// The pages the panel's actions act on, in reading order: the chosen ones,
/// or with none chosen the page being worked on.
pub(crate) fn targets(state: &TesseraApp) -> Vec<PageId> {
    let doc = state.active().document();
    let window = &state.pages_window;
    if window.of == Some(state.active) {
        let chosen: Vec<PageId> = doc
            .page_ids()
            .filter(|page| window.selected.contains(page))
            .collect();
        if !chosen.is_empty() {
            return chosen;
        }
    }
    state.current_page().into_iter().collect()
}

/// What a click on `page` makes the chosen pages, and where the next
/// Shift-click's run starts from.
///
/// A plain click chooses the page alone; Ctrl (Cmd on a Mac) adds it or
/// takes it out; Shift chooses every page from the last one clicked to this
/// one, in reading order, as a list anywhere else does.
fn select(
    chosen: &[PageId],
    anchor: Option<PageId>,
    page: PageId,
    order: &[PageId],
    modifiers: egui::Modifiers,
) -> (Vec<PageId>, Option<PageId>) {
    if modifiers.shift {
        let from = anchor.or_else(|| chosen.last().copied()).unwrap_or(page);
        let at = |p: PageId| order.iter().position(|o| *o == p);
        let (Some(a), Some(b)) = (at(from), at(page)) else {
            return (vec![page], Some(page));
        };
        (order[a.min(b)..=a.max(b)].to_vec(), Some(from))
    } else if modifiers.command {
        let mut chosen = chosen.to_vec();
        if let Some(at) = chosen.iter().position(|p| *p == page) {
            chosen.remove(at);
        } else {
            chosen.push(page);
        }
        (chosen, Some(page))
    } else {
        (vec![page], Some(page))
    }
}

// --- the parent pages ------------------------------------------------------

/// A parent's letter, for the corner of a page built on it: "A" of
/// "A-Master", as InDesign's prefix; the first letter of a name with none.
pub(crate) fn prefix(name: &str) -> String {
    match name.split_once('-') {
        Some((before, _)) if (1..=4).contains(&before.chars().count()) => before.to_string(),
        _ => name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default(),
    }
}

/// What a click, a double-click or a parent's menu asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParentAct {
    /// Build the chosen pages on it.
    Apply(Option<MasterId>),
    /// Build every page on it.
    ApplyAll(MasterId),
    /// Open it on the canvas, or close it if it is open.
    Edit(MasterId),
    Rename(MasterId),
    Delete(MasterId),
}

/// The parent pages, above the document's own: InDesign's arrangement.
///
/// Each row is the parent — its pages drawn small, its name, and how many
/// pages are built on it — and the one the chosen pages are built on is
/// marked. A click puts it on the chosen pages, a double-click opens it on
/// the canvas to edit, and it can be dragged onto any page in the list.
fn parents(ui: &mut Ui, state: &mut TesseraApp) {
    ui.horizontal(|ui| {
        crate::view::panels::group_label_pub(ui, "Parent pages");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::view::panels::icon_button(ui, Icon::Plus, "New parent page", false) {
                apply(state, Command::AddMaster);
            }
        });
    });

    let targets = targets(state);
    let doc = state.active().document();
    let rows: Vec<(Option<MasterId>, String, Vec<PageId>, usize)> =
        std::iter::once((None, "[None]".to_string(), Vec::new()))
            .chain(doc.master_ids().filter_map(|id| {
                let master = doc.masters.get(id)?;
                Some((Some(id), master.name.clone(), doc.pages_of_master(id)))
            }))
            .map(|(id, name, pages)| {
                let uses = doc
                    .page_ids()
                    .filter(|page| doc.master_of_page(*page) == id)
                    .count();
                (id, name, pages, uses)
            })
            .collect();
    let on_targets: Option<Option<MasterId>> = {
        let parents: Vec<Option<MasterId>> = targets
            .iter()
            .map(|page| doc.master_of_page(*page))
            .collect();
        parents
            .first()
            .copied()
            .filter(|first| parents.iter().all(|p| p == first))
    };
    let editing = state.editing_master;
    let mut act: Option<ParentAct> = None;
    let mut budget = 1;

    for (id, name, pages, uses) in rows {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), PARENT_ROW),
            Sense::click_and_drag(),
        );
        let on = on_targets == Some(id);
        let open = id.is_some() && editing == id;
        {
            let painter = ui.painter();
            if open {
                painter.rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
                painter.rect_stroke(
                    rect,
                    Theme::RADIUS,
                    Stroke::new(1.0, Theme::accent()),
                    egui::StrokeKind::Inside,
                );
            } else if on {
                painter.rect_filled(rect, Theme::RADIUS, Theme::accent_soft());
            } else if response.hovered() {
                painter.rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
            }
        }

        // Its pages, small: the parent's layout is what tells two of them
        // apart, far more than "A-Master" and "B-Master" do.
        let mut x = rect.left() + 6.0;
        let aspect = {
            let first = state.active().document().first_page_bounds();
            (first.height / first.width.max(1.0)) as f32
        };
        let size = Vec2::new(PARENT_PAGE, (PARENT_PAGE * aspect).min(PARENT_ROW - 6.0));
        let top = rect.center().y - size.y / 2.0;
        match id {
            None => {
                let sheet = Rect::from_min_size(egui::pos2(x + PARENT_PAGE / 2.0, top), size);
                ui.painter().rect_stroke(
                    sheet,
                    1.0,
                    Stroke::new(1.0, Theme::rule()),
                    egui::StrokeKind::Inside,
                );
                ui.painter().line_segment(
                    [sheet.left_bottom(), sheet.right_top()],
                    Stroke::new(1.0, Theme::rule()),
                );
            }
            Some(master) => {
                for page in pages.iter().take(2) {
                    let sheet = Rect::from_min_size(egui::pos2(x, top), size);
                    thumbnail(ui, state, *page, sheet, Scope::Master(master), &mut budget);
                    ui.painter().rect_stroke(
                        sheet,
                        1.0,
                        Stroke::new(1.0, Theme::border()),
                        egui::StrokeKind::Inside,
                    );
                    x += PARENT_PAGE + 1.0;
                }
            }
        }
        let text_left = rect.left() + 6.0 + 2.0 * PARENT_PAGE + 10.0;

        // The count first, at the right, so the name knows its room.
        let count = match uses {
            0 => "unused".to_string(),
            1 => "1 page".to_string(),
            n => format!("{n} pages"),
        };
        let count = ui.painter().layout_no_wrap(
            count,
            egui::TextStyle::Small.resolve(ui.style()),
            Theme::text_muted(),
        );
        let count_left = rect.right() - 8.0 - count.size().x;
        ui.painter().galley(
            egui::pos2(count_left, rect.center().y - count.size().y / 2.0),
            count,
            Theme::text_muted(),
        );

        let renaming = match (&state.pages_window.renaming, id) {
            (Some((r, text)), Some(master)) if *r == master => Some(text.clone()),
            _ => None,
        };
        if let (Some(mut text), Some(master)) = (renaming, id) {
            let field = Rect::from_min_max(
                egui::pos2(text_left - 3.0, rect.top() + 4.0),
                egui::pos2(count_left - 6.0, rect.bottom() - 4.0),
            );
            let edit = ui.put(
                field,
                egui::TextEdit::singleline(&mut text).id(egui::Id::new(("rename-parent", master))),
            );
            let edit = crate::icons::speak_as(edit, "Parent page name");
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if edit.lost_focus() || cancel {
                state.pages_window.renaming = None;
                let name = text.trim();
                let taken = state
                    .active()
                    .document()
                    .masters
                    .iter()
                    .any(|(other, m)| other != master && m.name == name);
                if !cancel && !name.is_empty() && !taken && name != state_name(state, master) {
                    apply(
                        state,
                        Command::RenameMaster {
                            id: master,
                            name: name.to_owned(),
                        },
                    );
                }
            } else {
                state.pages_window.renaming = Some((master, text));
            }
        } else {
            let mut job = egui::text::LayoutJob::simple_singleline(
                name.clone(),
                egui::TextStyle::Body.resolve(ui.style()),
                if id.is_some() {
                    Theme::text_primary()
                } else {
                    Theme::text_muted()
                },
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(
                (count_left - text_left - 8.0).max(8.0),
            );
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(
                egui::pos2(text_left, rect.center().y - galley.size().y / 2.0),
                galley,
                Theme::text_primary(),
            );
        }

        let label = match id {
            None => "No parent".to_string(),
            Some(_) => name.clone(),
        };
        let response =
            crate::icons::reads_as(response, &label, egui::WidgetType::RadioButton, Some(on))
                .on_hover_text(match id {
                    None => "Click to take the chosen pages off their parent, or drag onto a page",
                    Some(_) => {
                        "Click to build the chosen pages on it, double-click to edit it, \
                         or drag it onto a page"
                    }
                });
        response.dnd_set_drag_payload(ParentPayload(id));
        if response.dragged() {
            dragging_label(ui, &label);
        }
        if response.double_clicked() {
            if let Some(master) = id {
                act = Some(ParentAct::Edit(master));
            }
        } else if response.clicked() {
            act = Some(ParentAct::Apply(id));
        }
        if let Some(master) = id {
            response.context_menu(|ui| {
                let open_label = if open { "Close parent" } else { "Edit parent" };
                for (label, choice) in [
                    (open_label, ParentAct::Edit(master)),
                    ("Rename…", ParentAct::Rename(master)),
                    ("Apply to all pages", ParentAct::ApplyAll(master)),
                    ("Delete parent", ParentAct::Delete(master)),
                ] {
                    if ui.button(label).clicked() {
                        act = Some(choice);
                        ui.close();
                    }
                }
            });
        }
    }

    match act {
        Some(ParentAct::Apply(master)) if !targets.is_empty() => apply(
            state,
            Command::ApplyMasterToPages {
                pages: targets,
                master,
            },
        ),
        Some(ParentAct::ApplyAll(master)) => apply(
            state,
            Command::ApplyMasterToAll {
                master: Some(master),
            },
        ),
        Some(ParentAct::Edit(master)) => {
            // Toggling: double-clicking the parent already open closes it,
            // so the way in is the way out, as well as the bar on the canvas.
            let now = (state.editing_master != Some(master)).then_some(master);
            state.edit_master(now);
        }
        Some(ParentAct::Rename(master)) => {
            let name = state_name(state, master).to_owned();
            state.pages_window.renaming = Some((master, name));
        }
        Some(ParentAct::Delete(master)) => {
            if state.editing_master == Some(master) {
                state.edit_master(None);
            }
            apply(state, Command::RemoveMaster { id: master });
        }
        _ => {}
    }
}

/// A parent's name as the document has it.
fn state_name(state: &TesseraApp, master: MasterId) -> &str {
    state
        .active()
        .document()
        .masters
        .get(master)
        .map_or("", |m| m.name.as_str())
}

/// The name of what is being dragged, beside the pointer, so a drag across
/// the list says what it will drop.
pub(crate) fn dragging_label(ui: &Ui, label: &str) {
    let Some(at) = ui.ctx().pointer_interact_pos() else {
        return;
    };
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("pages-drag-label"),
    ));
    let galley = painter.layout_no_wrap(
        label.to_owned(),
        egui::TextStyle::Small.resolve(ui.style()),
        Theme::text_primary(),
    );
    let pill = Rect::from_min_size(
        at + Vec2::new(14.0, 10.0),
        galley.size() + Vec2::new(14.0, 6.0),
    );
    painter.rect(
        pill,
        pill.height() / 2.0,
        Theme::panel_bg_solid(),
        Stroke::new(1.0, Theme::accent_edge()),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        pill.min + Vec2::new(7.0, 3.0),
        galley,
        Theme::text_primary(),
    );
}

// --- the pages ---------------------------------------------------------------

/// "Pages", how many there are, and how large to draw them.
fn heading(ui: &mut Ui, state: &mut TesseraApp) {
    let count = state.active().document().page_ids().count();
    ui.horizontal(|ui| {
        crate::view::panels::group_label_pub(ui, "Pages");
        ui.colored_label(
            Theme::text_muted(),
            if count == 1 {
                "1 page".to_string()
            } else {
                format!("{count} pages")
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            size_choice(ui, state);
        });
    });
}

/// Three sizes, each drawn as a page of that size: larger to read a layout,
/// smaller to see a whole book.
fn size_choice(ui: &mut Ui, state: &mut TesseraApp) {
    ui.spacing_mut().item_spacing.x = 1.0;
    // Right to left, so the largest is drawn first.
    for (size, side, name) in [
        (ThumbnailSize::Large, 12.0, "Large pages"),
        (ThumbnailSize::Medium, 9.0, "Medium pages"),
        (ThumbnailSize::Small, 6.0, "Small pages"),
    ] {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
        let chosen = state.pages_window.size == size;
        let painter = ui.painter();
        if chosen {
            painter.rect_filled(rect, 4.0, Theme::selected_bg());
        } else if response.hovered() {
            painter.rect_filled(rect, 4.0, Theme::hover_bg());
        }
        let page = Rect::from_center_size(rect.center(), Vec2::new(side, side * 1.3));
        painter.rect_stroke(
            page,
            1.0,
            Stroke::new(
                1.2,
                if chosen {
                    Theme::text_primary()
                } else {
                    Theme::text_muted()
                },
            ),
            egui::StrokeKind::Inside,
        );
        let response =
            crate::icons::reads_as(response, name, egui::WidgetType::RadioButton, Some(chosen))
                .on_hover_text(name);
        if response.clicked() {
            state.pages_window.size = size;
        }
    }
}

/// A page as the list lays it out: which spread, in reading order, and
/// where.
#[derive(Clone, Copy, Debug)]
struct Placed {
    page: PageId,
    spread: usize,
    rect: Rect,
}

/// A page to be laid out: its spread, the column of the spread it sits in —
/// 0 the verso, 1 the recto, for facing pages — and its size as drawn.
#[derive(Clone, Copy, Debug)]
struct Slot {
    spread: usize,
    column: f32,
    size: Vec2,
}

/// Where each page goes in a list `width` wide, in reading order, and how
/// tall the list is.
///
/// Facing pages: a row per spread, a verso's right edge and a recto's left
/// edge against one spine down the middle. Pages that do not face: as many
/// to a row as fit, the rows centred.
fn arrange(slots: &[Slot], facing: bool, width: f32) -> (Vec<Rect>, f32) {
    let mut rects = Vec::with_capacity(slots.len());
    let mut y = 0.0;
    if facing {
        let spine = width / 2.0;
        let mut at = 0;
        while at < slots.len() {
            let spread = slots[at].spread;
            let row: Vec<&Slot> = slots[at..]
                .iter()
                .take_while(|slot| slot.spread == spread)
                .collect();
            let tallest = row.iter().map(|slot| slot.size.y).fold(0.0, f32::max);
            for slot in &row {
                let x = if slot.column < 1.0 {
                    spine - FOLD / 2.0 - slot.size.x
                } else {
                    spine + FOLD / 2.0 + (slot.column - 1.0) * (slot.size.x + FOLD)
                };
                rects.push(Rect::from_min_size(egui::pos2(x, y + MARK), slot.size));
            }
            y += MARK + tallest + LABEL + ROW_GAP;
            at += row.len();
        }
    } else {
        let cell = slots.iter().map(|slot| slot.size.x).fold(0.0, f32::max);
        let across = (((width - 2.0 * PAD + GRID_GAP) / (cell + GRID_GAP)).floor() as usize).max(1);
        for row in slots.chunks(across) {
            let block = row.len() as f32 * cell + (row.len() as f32 - 1.0) * GRID_GAP;
            let left = (width - block) / 2.0;
            let tallest = row.iter().map(|slot| slot.size.y).fold(0.0, f32::max);
            for (i, slot) in row.iter().enumerate() {
                let x = left + i as f32 * (cell + GRID_GAP) + (cell - slot.size.x) / 2.0;
                rects.push(Rect::from_min_size(egui::pos2(x, y + MARK), slot.size));
            }
            y += MARK + tallest + LABEL + ROW_GAP;
        }
    }
    (rects, y)
}

/// Every document page, laid out for a list `width` wide.
fn placed(state: &TesseraApp, width: f32) -> (Vec<Placed>, f32) {
    let doc = state.active().document();
    let facing = doc.setup.facing_pages;
    let columns = if facing { 2.0 } else { 1.0 };
    // One scale for every page, so a wider page — a cover, a gatefold — is
    // drawn wider rather than squeezed to the width of the rest.
    let most = ((width - 2.0 * PAD - (columns - 1.0) * FOLD) / columns).max(12.0);
    let page_width = state.pages_window.size.width().min(most);
    let scale = f64::from(page_width) / doc.first_page_bounds().width.max(1.0);
    let mut pages = Vec::new();
    let mut slots = Vec::new();
    for (index, spread) in doc.spread_order.iter().enumerate() {
        for (column, page) in doc.pages_of(*spread).into_iter().enumerate() {
            let Some(bounds) = doc.pages.get(page).map(|p| p.bounds) else {
                continue;
            };
            let size = Vec2::new(
                ((bounds.width * scale) as f32).min(most),
                (bounds.height * scale) as f32,
            );
            slots.push(Slot {
                spread: index,
                column: column_of(state, *spread, column, facing),
                size,
            });
            pages.push(page);
        }
    }
    let (rects, height) = arrange(&slots, facing, width);
    let placed = pages
        .into_iter()
        .zip(slots)
        .zip(rects)
        .map(|((page, slot), rect)| Placed {
            page,
            spread: slot.spread,
            rect,
        })
        .collect();
    (placed, height)
}

/// What a page's menu, or the footer, asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PageAct {
    /// A new page after the last chosen one.
    Insert,
    Duplicate,
    Delete,
    Parent(Option<MasterId>),
    /// Take a page's local changes to its parent's items back.
    RemoveOverrides(PageId),
    /// The numbering and section options, opened on a page.
    Numbering(PageId),
    /// Insert pages as the form says.
    InsertMany(InsertForm),
    /// Open or close the Insert pages form.
    InsertForm,
    /// Give the chosen pages this size.
    Size(f64, f64),
    /// Turn the chosen pages a quarter: portrait to landscape, or back.
    Turn,
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    let width = ui.available_width();
    let (placed, height) = placed(state, width);
    let (area, _) = ui.allocate_exact_size(Vec2::new(width, height.max(1.0)), Sense::hover());
    let placed: Vec<Placed> = placed
        .into_iter()
        .map(|p| Placed {
            rect: p.rect.translate(area.min.to_vec2()),
            ..p
        })
        .collect();

    let key = state.active;
    let doc = state.active().document();
    let order: Vec<PageId> = placed.iter().map(|p| p.page).collect();
    let current = state
        .active()
        .current_spread
        .min(doc.spread_order.len().saturating_sub(1));
    let chosen = targets(state);
    let explicit = !state.pages_window.selected.is_empty();
    let masters: Vec<(MasterId, String)> = doc
        .master_ids()
        .filter_map(|id| doc.masters.get(id).map(|m| (id, m.name.clone())))
        .collect();
    let unit = state.prefs.unit;
    let facts: Vec<(String, Option<String>, bool, bool, bool)> = order
        .iter()
        .map(|page| {
            let parent = doc
                .master_of_page(*page)
                .and_then(|m| masters.iter().find(|(id, _)| *id == m))
                .map(|(_, name)| name.clone());
            let has_parent = parent.is_some();
            let landscape = doc
                .pages
                .get(*page)
                .is_some_and(|p| p.bounds.width > p.bounds.height);
            (
                doc.page_label(*page).unwrap_or_default(),
                parent,
                doc.starts_section(*page),
                has_parent,
                landscape,
            )
        })
        .collect();

    // Follow the canvas: turned to another spread from anywhere else — the
    // status bar, a Next use, a find — the list brings it into view.
    if state.pages_window.shown != Some((key, current)) {
        if let Some(rect) = placed
            .iter()
            .filter(|p| p.spread == current)
            .map(|p| p.rect)
            .reduce(|a, b| a.union(b))
        {
            ui.scroll_to_rect(rect.expand2(Vec2::new(0.0, MARK + LABEL)), None);
        }
        state.pages_window.shown = Some((key, current));
    }

    let mut budget = RENDERS_PER_FRAME;
    let mut clicked: Option<(PageId, egui::Modifiers)> = None;
    let mut dragging: Option<PageId> = None;
    let mut dropped = false;
    let mut parent_dropped: Option<(PageId, Option<MasterId>)> = None;
    let mut act: Option<PageAct> = None;
    let modifiers = ui.input(|i| i.modifiers);
    let slots: Vec<Rect> = placed.iter().map(|p| p.rect).collect();

    for (spot, (label, parent, section, has_parent, landscape)) in placed.iter().zip(facts) {
        let page = spot.page;
        let rect = spot.rect;
        let is_chosen = explicit && chosen.contains(&page);
        let is_current = spot.spread == current;
        let response = ui.interact(
            rect.expand(3.0),
            egui::Id::new(("page", page)),
            Sense::click_and_drag(),
        );
        let parent_over = response.dnd_hover_payload::<ParentPayload>().is_some();

        // Chosen: the page on the accent's ground, as a chosen row is.
        if is_chosen {
            ui.painter()
                .rect_filled(rect.expand(4.0), 5.0, Theme::accent_soft());
        }
        // A shadow, so white paper reads as paper on a light ground.
        ui.painter().rect_filled(
            rect.translate(Vec2::new(0.0, 1.5)),
            2.0,
            Color32::from_black_alpha(if Theme::is_light() { 34 } else { 70 }),
        );
        thumbnail(ui, state, page, rect, Scope::Document, &mut budget);
        let (edge, weight) = if is_chosen || parent_over {
            (Theme::accent(), 2.0)
        } else if response.hovered() {
            (Theme::accent_edge(), 1.5)
        } else {
            (Theme::border(), 1.0)
        };
        ui.painter().rect_stroke(
            rect,
            1.0,
            Stroke::new(weight, edge),
            egui::StrokeKind::Outside,
        );

        // Its parent's letter, in the corner, on a ground of its own so it
        // reads over whatever the page has there.
        if let Some(name) = &parent {
            let letter = prefix(name);
            let font = egui::FontId::proportional(9.5);
            let galley = ui
                .painter()
                .layout_no_wrap(letter, font, Theme::text_primary());
            let tag = Rect::from_min_size(
                rect.left_top() + Vec2::new(2.0, 2.0),
                galley.size() + Vec2::new(6.0, 2.0),
            );
            ui.painter().rect(
                tag,
                3.0,
                Theme::panel_bg_solid(),
                Stroke::new(1.0, Theme::rule()),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .galley(tag.min + Vec2::new(3.0, 1.0), galley, Theme::text_primary());
        }
        // A section starts here: InDesign's triangle over the page.
        if section {
            let apex = egui::pos2(rect.left() + 4.5, rect.top() - 2.0);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(rect.left(), rect.top() - MARK + 1.0),
                    egui::pos2(rect.left() + 9.0, rect.top() - MARK + 1.0),
                    apex,
                ],
                Theme::accent(),
                Stroke::NONE,
            ));
        }
        // The number, under the page; the pages the canvas shows on a pill.
        {
            let galley = ui.painter().layout_no_wrap(
                label.clone(),
                egui::TextStyle::Small.resolve(ui.style()),
                Theme::text_primary(),
            );
            let centre = egui::pos2(rect.center().x, rect.bottom() + LABEL / 2.0 + 1.0);
            if is_current {
                let pill = Rect::from_center_size(
                    centre,
                    Vec2::new(galley.size().x + 12.0, galley.size().y + 2.0),
                );
                ui.painter()
                    .rect_filled(pill, pill.height() / 2.0, Theme::selected_bg());
            }
            ui.painter().galley(
                centre - galley.size() / 2.0,
                galley,
                if is_current {
                    Theme::text_primary()
                } else {
                    Theme::text_muted()
                },
            );
        }

        let spoken = match &parent {
            Some(name) => format!("Page {label}, on {name}"),
            None => format!("Page {label}"),
        };
        let response = crate::icons::reads_as(
            response,
            format!("Page {label}"),
            egui::WidgetType::SelectableLabel,
            Some(is_chosen),
        )
        .on_hover_text(format!(
            "{spoken}. Click to go to it; Shift-click or Ctrl-click to choose several; \
             drag to move."
        ));

        if response.dragged() {
            dragging = Some(page);
        }
        if response.drag_stopped() {
            dropped = true;
            dragging = Some(page);
        }
        if response.clicked() {
            clicked = Some((page, modifiers));
        }
        if let Some(payload) = response.dnd_release_payload::<ParentPayload>() {
            parent_dropped = Some((page, payload.0));
        }
        if response.secondary_clicked() && !(explicit && chosen.contains(&page)) {
            // The menu acts on what is chosen, so the page it was opened on
            // is chosen first, as a right-click does in any list.
            clicked = Some((page, egui::Modifiers::NONE));
        }
        response.context_menu(|ui| {
            let many = explicit && chosen.len() > 1 && chosen.contains(&page);
            let (insert, duplicate, delete) = if many {
                ("Insert page after these", "Duplicate pages", "Delete pages")
            } else {
                ("Insert page after", "Duplicate page", "Delete page")
            };
            if ui.button(insert).clicked() {
                act = Some(PageAct::Insert);
                ui.close();
            }
            if ui.button(duplicate).clicked() {
                act = Some(PageAct::Duplicate);
                ui.close();
            }
            if ui
                .add_enabled(order.len() > chosen.len().max(1), egui::Button::new(delete))
                .clicked()
            {
                act = Some(PageAct::Delete);
                ui.close();
            }
            ui.separator();
            ui.menu_button("Parent page", |ui| {
                if ui.button("[None]").clicked() {
                    act = Some(PageAct::Parent(None));
                    ui.close();
                }
                for (id, name) in &masters {
                    if ui.button(name).clicked() {
                        act = Some(PageAct::Parent(Some(*id)));
                        ui.close();
                    }
                }
            });
            if has_parent && ui.button("Remove local overrides").clicked() {
                act = Some(PageAct::RemoveOverrides(page));
                ui.close();
            }
            ui.menu_button("Page size", |ui| {
                for preset in OFFERED_SIZES {
                    let (w, h) = preset.size();
                    if ui
                        .button(format!(
                            "{}  \u{00b7}  {}",
                            preset.name(),
                            size_measured(w, h, unit)
                        ))
                        .clicked()
                    {
                        act = Some(PageAct::Size(w, h));
                        ui.close();
                    }
                }
                ui.separator();
                if ui
                    .button(if landscape {
                        "Turn to portrait"
                    } else {
                        "Turn to landscape"
                    })
                    .clicked()
                {
                    act = Some(PageAct::Turn);
                    ui.close();
                }
            });
            if ui.button("Insert pages\u{2026}").clicked() {
                act = Some(PageAct::InsertForm);
                ui.close();
            }
            ui.separator();
            if ui.button("Numbering & section options…").clicked() {
                act = Some(PageAct::Numbering(page));
                ui.close();
            }
        });
    }

    // Moving the chosen pages when one of them is dragged, and the one page
    // otherwise.
    let moving: Vec<PageId> = match dragging {
        Some(page) if explicit && chosen.contains(&page) => chosen.clone(),
        Some(page) => vec![page],
        None => Vec::new(),
    };
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
    if dragging.is_some() && !dropped {
        let what = match moving.len() {
            1 => "1 page".to_string(),
            n => format!("{n} pages"),
        };
        dragging_label(ui, &what);
    }

    if let Some((page, modifiers)) = clicked {
        let window = &state.pages_window;
        let (now, anchor) = select(&window.selected, window.anchor, page, &order, modifiers);
        state.pages_window.selected = now;
        state.pages_window.anchor = anchor;
        // A plain click also turns the canvas to it, leaving a parent that
        // was open: the page is what was asked for.
        if !modifiers.shift && !modifiers.command {
            turn_to(state, page);
        }
    }
    if dropped
        && let Some(to) = landing
        && !moving.is_empty()
    {
        apply(state, Command::MovePages { ids: moving, to });
    }
    if let Some((page, master)) = parent_dropped {
        // Onto one of the chosen pages, it goes on all of them; onto any other
        // page, on that one.
        let pages = if explicit && chosen.contains(&page) {
            chosen.clone()
        } else {
            vec![page]
        };
        apply(state, Command::ApplyMasterToPages { pages, master });
    }
    if let Some(act) = act {
        run(state, act);
    }
}

/// Turn the canvas to the spread `page` is on, leaving any parent open.
pub(crate) fn turn_to(state: &mut TesseraApp, page: PageId) {
    let doc = state.active().document();
    let Some(at) = doc
        .spread_order
        .iter()
        .position(|spread| doc.pages_of(*spread).contains(&page))
    else {
        return;
    };
    if state.editing_master.is_some() {
        state.edit_master(None);
    }
    state.active_mut().current_spread = at;
    state.active_mut().fitted = false;
}

/// Do what a page's menu or the footer asked, to the chosen pages; and choose
/// what the action made, so the next action acts on it.
fn run(state: &mut TesseraApp, act: PageAct) {
    let pages = targets(state);
    let before: Vec<PageId> = state.active().document().page_ids().collect();
    let made = |state: &TesseraApp| -> Vec<PageId> {
        state
            .active()
            .document()
            .page_ids()
            .filter(|page| !before.contains(page))
            .collect()
    };
    match act {
        PageAct::Insert => {
            let after = pages.last().copied().or_else(|| before.last().copied());
            apply(state, Command::InsertPage { after });
            choose_made(state, made(state));
        }
        PageAct::Duplicate => {
            if pages.is_empty() {
                return;
            }
            apply(state, Command::DuplicatePages { ids: pages });
            choose_made(state, made(state));
        }
        PageAct::Delete => {
            if pages.is_empty() || pages.len() >= before.len() {
                return;
            }
            apply(state, Command::RemovePages { ids: pages });
            state.pages_window.selected.clear();
            state.pages_window.anchor = None;
            let spreads = state.active().document().spread_order.len();
            let open = state.active_mut();
            open.current_spread = open.current_spread.min(spreads.saturating_sub(1));
        }
        PageAct::Parent(master) => {
            if !pages.is_empty() {
                apply(state, Command::ApplyMasterToPages { pages, master });
            }
        }
        PageAct::RemoveOverrides(page) => apply(state, Command::RemoveOverrides { page }),
        PageAct::Numbering(page) => {
            let mut window = std::mem::take(&mut state.numbering);
            window.open(state.active().document(), Some(page));
            state.numbering = window;
        }
        PageAct::InsertMany(form) => {
            let after = match form.at {
                InsertAt::AfterChosen => pages.last().copied().or_else(|| before.last().copied()),
                InsertAt::Start => None,
                InsertAt::End => before.last().copied(),
            };
            let parent = match form.parent {
                InsertParent::AsBefore => None,
                InsertParent::None => Some(None),
                InsertParent::Master(id) => Some(Some(id)),
            };
            apply(
                state,
                Command::InsertPages {
                    after,
                    count: form.count.clamp(1, MOST_INSERTED),
                    parent,
                },
            );
            choose_made(state, made(state));
        }
        PageAct::InsertForm => {
            let window = &mut state.pages_window;
            window.inserting = match window.inserting {
                Some(_) => None,
                None => Some(InsertForm::default()),
            };
        }
        PageAct::Size(width, height) => {
            if pages.is_empty() {
                return;
            }
            apply(
                state,
                Command::Together(
                    pages
                        .iter()
                        .map(|page| Command::SetPageSizeOf {
                            page: *page,
                            width,
                            height,
                        })
                        .collect(),
                ),
            );
        }
        PageAct::Turn => {
            let doc = state.active().document();
            let turned: Vec<Command> = pages
                .iter()
                .filter_map(|page| {
                    let b = doc.pages.get(*page)?.bounds;
                    Some(Command::SetPageSizeOf {
                        page: *page,
                        width: b.height,
                        height: b.width,
                    })
                })
                .collect();
            if !turned.is_empty() {
                apply(state, Command::Together(turned));
            }
        }
    }
}

/// A page size as a person names it: "A4", "Letter landscape", or, for a
/// size no preset has, its measurements in the unit being worked in.
fn size_name(width: f64, height: f64, unit: tessera_geometry::Unit) -> String {
    use tessera_document::nodes::PagePreset;
    let near = |a: f64, b: f64| (a - b).abs() < 0.5;
    for preset in PagePreset::ALL {
        let (w, h) = preset.size();
        if near(width, w) && near(height, h) {
            return preset.name().to_string();
        }
        if near(width, h) && near(height, w) {
            return format!("{} landscape", preset.name());
        }
    }
    size_measured(width, height, unit)
}

/// A size in the unit being worked in: "210 × 297 mm".
fn size_measured(width: f64, height: f64, unit: tessera_geometry::Unit) -> String {
    let number = |points: f64| {
        let said = unit.format(points);
        said.trim_end_matches(unit.suffix()).trim().to_string()
    };
    format!(
        "{} \u{00d7} {} {}",
        number(width),
        number(height),
        unit.suffix()
    )
}

/// The sizes offered for pages, in the order people reach for them.
const OFFERED_SIZES: [tessera_document::nodes::PagePreset; 7] = [
    tessera_document::nodes::PagePreset::A4,
    tessera_document::nodes::PagePreset::Letter,
    tessera_document::nodes::PagePreset::A5,
    tessera_document::nodes::PagePreset::A3,
    tessera_document::nodes::PagePreset::Legal,
    tessera_document::nodes::PagePreset::Tabloid,
    tessera_document::nodes::PagePreset::Executive,
];

/// Choose the pages an action made, and turn to the first of them.
fn choose_made(state: &mut TesseraApp, made: Vec<PageId>) {
    let Some(first) = made.first().copied() else {
        return;
    };
    state.pages_window.anchor = Some(first);
    state.pages_window.selected = made;
    turn_to(state, first);
}

/// What the actions will act on, in words: "Page 4", "Pages 2–3",
/// "3 pages".
fn summary(state: &TesseraApp) -> String {
    let pages = targets(state);
    let doc = state.active().document();
    let label = |page: &PageId| doc.page_label(*page).unwrap_or_default();
    let order: Vec<PageId> = doc.page_ids().collect();
    let run = pages
        .iter()
        .filter_map(|p| order.iter().position(|o| o == p))
        .collect::<Vec<_>>()
        .windows(2)
        .all(|w| w[1] == w[0] + 1);
    match pages.as_slice() {
        [] => "No page chosen".to_string(),
        [one] => format!("Page {}", label(one)),
        [first, .., last] if run => format!("Pages {}–{}", label(first), label(last)),
        many => format!("{} pages", many.len()),
    }
}

/// The size the chosen pages are, or that they differ.
fn sizes_said(state: &TesseraApp, pages: &[PageId]) -> String {
    let doc = state.active().document();
    let sizes: Vec<(f64, f64)> = pages
        .iter()
        .filter_map(|p| doc.pages.get(*p).map(|p| (p.bounds.width, p.bounds.height)))
        .collect();
    let Some(first) = sizes.first().copied() else {
        return String::new();
    };
    if sizes
        .iter()
        .all(|s| (s.0 - first.0).abs() < 0.5 && (s.1 - first.1).abs() < 0.5)
    {
        size_name(first.0, first.1, state.prefs.unit)
    } else {
        "sizes differ".to_string()
    }
}

/// The foot of the panel: what is chosen, and insert, duplicate and delete.
fn footer(ui: &mut Ui, state: &mut TesseraApp) {
    ui.add_space(2.0);
    let rule = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        rule,
        Stroke::new(1.0, Theme::rule()),
    );
    ui.add_space(4.0);
    let pages = targets(state);
    let count = state.active().document().page_ids().count();
    let text = format!("{} \u{00b7} {}", summary(state), sizes_said(state, &pages));
    let mut act = None;
    ui.horizontal(|ui| {
        // What is chosen, and what size it is, beside the buttons: as much as
        // the buttons leave, cut short rather than pushing them off.
        let room = (ui.available_width() - 4.0 * (Theme::control_height() + 4.0)).max(40.0);
        ui.allocate_ui(Vec2::new(room, Theme::control_height()), |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(&text).color(Theme::text_muted()))
                    .truncate()
                    .selectable(false),
            )
            .on_hover_text(&text);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let many = pages.len() > 1;
            ui.add_enabled_ui(!pages.is_empty() && pages.len() < count, |ui| {
                if crate::view::panels::icon_button(
                    ui,
                    Icon::Trash,
                    if many { "Delete pages" } else { "Delete page" },
                    false,
                ) {
                    act = Some(PageAct::Delete);
                }
            });
            ui.add_enabled_ui(!pages.is_empty(), |ui| {
                if crate::view::panels::icon_button(
                    ui,
                    Icon::Duplicate,
                    if many {
                        "Duplicate pages"
                    } else {
                        "Duplicate page"
                    },
                    false,
                ) {
                    act = Some(PageAct::Duplicate);
                }
            });
            if crate::view::panels::icon_button(ui, Icon::Plus, "Insert page", false) {
                act = Some(PageAct::Insert);
            }
            if crate::view::panels::icon_button(
                ui,
                Icon::AddFile,
                "Insert pages\u{2026}",
                state.pages_window.inserting.is_some(),
            ) {
                act = Some(PageAct::InsertForm);
            }
        });
    });
    if let Some(act) = act {
        run(state, act);
    }
}

/// Which place in the reading order a drop at `p` means.
///
/// Counted rather than hit-tested: a page goes *after* every slot the pointer
/// is past, where past means a row below, or the same row and beyond the
/// middle of the page. Dropping onto the right half of page four means five.
fn landing(p: egui::Pos2, slots: &[Rect]) -> usize {
    slots
        .iter()
        .filter(|r| p.y > r.bottom() || (p.y >= r.top() && p.x > r.center().x))
        .count()
}

/// Where to draw the line for a drop at `at`.
///
/// On the leading edge of the slot it would take, or the trailing edge of the
/// last one when it goes at the end.
fn marker(at: usize, slots: &[Rect]) -> Option<Rect> {
    let (slot, edge) = match slots.get(at) {
        Some(slot) => (slot, slot.left()),
        None => {
            let slot = slots.last()?;
            (slot, slot.right())
        }
    };
    Some(Rect::from_min_size(
        egui::pos2(edge - MARKER / 2.0, slot.top()),
        Vec2::new(MARKER, slot.height()),
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

/// A page in the panel: the page itself, drawn by the canvas's renderer
/// from the layout of `scope` — the document's for its pages, a parent's
/// for the parent's — filling `sheet`. Without a GPU (a test, a machine with
/// none) the boxes are drawn instead.
fn thumbnail(
    ui: &Ui,
    state: &mut TesseraApp,
    page: PageId,
    sheet: Rect,
    scope: Scope,
    budget: &mut u8,
) {
    let Some(bounds) = state.active().document().pages.get(page).map(|p| p.bounds) else {
        return;
    };
    let painter = ui.painter_at(sheet);
    if !rendered(ui, state, page, bounds, sheet, scope, budget, &painter) {
        schematic(&painter, state, page, bounds, sheet);
    }
}

/// The page, rendered, into `sheet`. `false` when there is no GPU to render
/// on, and nothing was drawn.
#[allow(clippy::too_many_arguments)]
fn rendered(
    ui: &Ui,
    state: &mut TesseraApp,
    page: PageId,
    bounds: tessera_geometry::DocRect,
    sheet: Rect,
    scope: Scope,
    budget: &mut u8,
    painter: &egui::Painter,
) -> bool {
    use std::hash::{Hash, Hasher};
    let Some(gpu) = ui
        .ctx()
        .data(|d| d.get_temp::<eframe::egui_wgpu::RenderState>(egui::Id::new(GPU)))
    else {
        return false;
    };
    let ppp = ui.ctx().pixels_per_point();
    let size = (
        (sheet.width() * ppp).round().max(1.0) as u32,
        (sheet.height() * ppp).round().max(1.0) as u32,
    );
    // Per document and page: two open documents can hold pages with the same
    // key, and one's thumbnail must not stand in for the other's.
    let key = {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (state.active, page).hash(&mut hasher);
        hasher.finish()
    };
    let revision = state.active().document().revision();
    let zoom = f64::from(size.0) / bounds.width.max(1.0);
    let drawn = crate::view::vello_host::thumbnail(&gpu, key, revision, size, *budget > 0, || {
        // The layout of the scope the page belongs to, whatever the canvas
        // is showing. Drawn from the canvas's, every page went blank at the
        // first change made while a parent was open: the parent's layout has
        // nothing on the document's pages.
        let resolved = state.resolve_in(scope).clone();
        tessera_render::scene::build_scene_with_images(
            &resolved,
            tessera_geometry::ViewTransform {
                pan: tessera_geometry::DocPoint {
                    x: bounds.x,
                    y: bounds.y,
                },
                zoom,
            },
            tessera_render::scene::SceneOptions {
                rules: false,
                clip: Some(vec![bounds]),
            },
            &mut state.images,
        )
    });
    let Some(drawn) = drawn else {
        return false;
    };
    if drawn.rendered {
        *budget = budget.saturating_sub(1);
    }
    if !drawn.current {
        // Its turn comes on a later frame; ask for one.
        ui.ctx().request_repaint();
    }
    painter.image(
        drawn.texture,
        sheet,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    true
}

/// The page as boxes: each object a block of its fill colour, for when there
/// is no GPU to draw the page itself.
fn schematic(
    painter: &egui::Painter,
    state: &TesseraApp,
    page: PageId,
    bounds: tessera_geometry::DocRect,
    at: Rect,
) {
    let doc = state.active().document();
    let scale = f64::from(at.width()) / bounds.width.max(1.0);
    painter.rect_filled(at, 1.0, Color32::WHITE);

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
        let block = Rect::from_min_size(
            at.min
                + Vec2::new(
                    ((b.x - bounds.x) * scale) as f32,
                    ((b.y - bounds.y) * scale) as f32,
                ),
            Vec2::new((b.width * scale) as f32, (b.height * scale) as f32),
        );
        // A thumbnail block is a few pixels of one colour. Drawing the whole
        // ramp at that size would cost a gradient per object for something
        // nobody can see, so it takes one colour from it.
        let [r, g, bl, a] = doc
            .resolve_colour(&frame.fill.representative())
            .to_rgb_f32();
        painter.rect_filled(
            block.intersect(at),
            0.0,
            Color32::from_rgba_unmultiplied(
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
        assert_eq!(
            named,
            vec![
                "Pages",
                "Layers",
                "Swatches",
                "Preflight",
                "AI Console",
                "Glyphs",
                "Book",
                "Links"
            ]
        );
        assert_eq!(Group::Window.menu(), Some("Window"));
    }

    #[test]
    fn opening_the_panel_is_not_an_edit() {
        let mut state = TesseraApp::headless();
        assert!(!state.active().dirty);
        actions::run(&mut state, Run::TogglePages);
        assert!(!state.active().dirty, "a panel is not a change to the work");
    }

    /// Choose the pages of the `index`th spread, as a click and a
    /// Shift-click would.
    fn choose_spread(state: &mut TesseraApp, index: usize) {
        let spread = state.active().document().spread_order[index];
        state.pages_window.selected = state.active().document().pages_of(spread);
        state.pages_window.of = Some(state.active);
    }

    #[test]
    fn a_lone_page_is_numbered_and_a_facing_pair_is_a_range() {
        // With an en dash. The panel drew the dash as three characters of
        // mangled text: the file had been read as Latin-1 and written back
        // as UTF-8, and this test checked for the mangled form.
        let mut state = TesseraApp::headless();
        state.active_mut().document_mut().setup.facing_pages = true;
        apply(&mut state, Command::AddPage);
        apply(&mut state, Command::AddPage);

        choose_spread(&mut state, 0);
        assert_eq!(summary(&state), "Page 1");
        choose_spread(&mut state, 1);
        assert_eq!(
            summary(&state),
            "Pages 2\u{2013}3",
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
        choose_spread(&mut state, 2);
        assert_eq!(summary(&state), "Page 3");

        apply(&mut state, Command::MoveSpread { from: 2, to: 0 });
        state.pages_window.selected = state.active().document().pages_of(order[2]);
        assert_eq!(summary(&state), "Page 1", "the same spread is page one now");
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

    fn slot(x: f32, y: f32) -> Rect {
        Rect::from_min_size(egui::pos2(x, y), egui::vec2(46.0, 60.0))
    }

    /// Two spreads: page one alone on the right, then two facing.
    fn a_short_document() -> Vec<Rect> {
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

    // --- choosing pages -------------------------------------------------------

    fn ids(n: usize) -> Vec<PageId> {
        let mut doc = tessera_document::Document::new();
        while doc.page_ids().count() < n {
            doc.add_page();
        }
        doc.page_ids().collect()
    }

    #[test]
    fn a_click_chooses_one_ctrl_adds_one_and_shift_chooses_a_run() {
        let order = ids(6);
        let plain = egui::Modifiers::NONE;
        let (chosen, anchor) = select(&[], None, order[2], &order, plain);
        assert_eq!((chosen.clone(), anchor), (vec![order[2]], Some(order[2])));

        let (chosen, anchor) = select(&chosen, anchor, order[4], &order, egui::Modifiers::COMMAND);
        assert_eq!(chosen, [order[2], order[4]]);
        let (chosen, anchor) = select(&chosen, anchor, order[2], &order, egui::Modifiers::COMMAND);
        assert_eq!(chosen, [order[4]], "a second Ctrl-click takes it out");

        // From the last page clicked — the third, Ctrl-clicked out — and
        // backwards as well as forwards.
        let (chosen, anchor) = select(&chosen, anchor, order[1], &order, egui::Modifiers::SHIFT);
        assert_eq!(chosen, order[1..=2]);
        let (chosen, _) = select(&chosen, anchor, order[5], &order, egui::Modifiers::SHIFT);
        assert_eq!(
            chosen,
            order[2..=5],
            "a second Shift-click runs from the same page"
        );
    }

    // --- laying the pages out --------------------------------------------------

    fn laid(spread: usize, column: f32) -> Slot {
        Slot {
            spread,
            column,
            size: Vec2::new(40.0, 52.0),
        }
    }

    #[test]
    fn facing_pages_meet_at_one_spine_down_the_list() {
        // Page one alone on the right, then pairs: every recto starts at the
        // spine and every verso ends there, so the book reads as a book.
        let (rects, height) = arrange(
            &[laid(0, 1.0), laid(1, 0.0), laid(1, 1.0), laid(2, 0.0)],
            true,
            200.0,
        );
        let spine = 100.0;
        assert!((rects[0].left() - (spine + FOLD / 2.0)).abs() < 0.01);
        assert!((rects[1].right() - (spine - FOLD / 2.0)).abs() < 0.01);
        assert!((rects[2].left() - (spine + FOLD / 2.0)).abs() < 0.01);
        assert!(
            (rects[3].right() - (spine - FOLD / 2.0)).abs() < 0.01,
            "a last lone verso"
        );
        assert_eq!(rects[1].top(), rects[2].top(), "a spread is one row");
        assert!(
            rects[3].top() > rects[1].bottom(),
            "and the next is under it"
        );
        assert!(height > rects[3].bottom());
    }

    #[test]
    fn pages_that_do_not_face_fill_rows_across_the_list() {
        // One to a row down the left edge was a column of pages beside an
        // empty panel.
        let slots: Vec<Slot> = (0..7).map(|i| laid(i, 0.0)).collect();
        let (rects, _) = arrange(&slots, false, 200.0);
        let first_row: Vec<&Rect> = rects.iter().filter(|r| r.top() == rects[0].top()).collect();
        assert_eq!(first_row.len(), 3, "as many as fit across 200 points");
        let (left, right) = (first_row[0].left(), 200.0 - first_row[2].right());
        assert!((left - right).abs() < 0.01, "the row is centred");
        assert!(
            rects[6].top() > rects[3].top(),
            "the seventh starts the third row"
        );
    }

    // --- the panel, used ------------------------------------------------------

    fn panel(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, Rect)> {
        let output = crate::headless_frame::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(300.0, 1400.0),
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
                            Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn a_panel() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    fn at(ctx: &egui::Context, state: &mut TesseraApp, label: &str) -> egui::Pos2 {
        panel(ctx, state, Vec::new());
        let nodes = panel(ctx, state, Vec::new());
        nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
            .center()
    }

    fn press(pos: egui::Pos2, pressed: bool, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers,
            },
        ]
    }

    fn click_with(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        label: &str,
        modifiers: egui::Modifiers,
    ) {
        let pos = at(ctx, state, label);
        for pressed in [true, false] {
            let mut events = vec![egui::Event::ModifiersChanged(modifiers)];
            events.extend(press(pos, pressed, modifiers));
            panel(ctx, state, events);
        }
        panel(
            ctx,
            state,
            vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)],
        );
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        click_with(ctx, state, label, egui::Modifiers::NONE);
    }

    /// A facing document of six pages: 1 | 2-3 | 4-5 | 6.
    fn six_pages() -> TesseraApp {
        let mut state = TesseraApp::headless();
        state.active_mut().document_mut().setup.facing_pages = true;
        for _ in 0..5 {
            apply(&mut state, Command::AddPage);
        }
        state
    }

    fn page(state: &TesseraApp, n: usize) -> PageId {
        state
            .active()
            .document()
            .page_ids()
            .nth(n - 1)
            .expect("a page")
    }

    #[test]
    fn a_click_chooses_a_page_and_turns_to_it_and_shift_chooses_a_run() {
        let mut state = six_pages();
        let ctx = a_panel();
        click(&ctx, &mut state, "Page 5");
        assert_eq!(targets(&state), [page(&state, 5)]);
        assert_eq!(state.active().current_spread, 2, "turned to 4–5");

        click_with(&ctx, &mut state, "Page 2", egui::Modifiers::SHIFT);
        assert_eq!(
            targets(&state),
            [
                page(&state, 2),
                page(&state, 3),
                page(&state, 4),
                page(&state, 5)
            ]
        );
        assert_eq!(state.active().current_spread, 2, "Shift only chooses");
    }

    #[test]
    fn a_parent_goes_on_the_chosen_pages_right_hand_ones_included() {
        // The panel put a parent on "the current page": the first page of the
        // spread in view. A recto could not be given one from here at all.
        let mut state = six_pages();
        apply(&mut state, Command::AddMaster);
        let master = state.active().document().master_order[0];
        let ctx = a_panel();
        click(&ctx, &mut state, "Page 3");
        click(&ctx, &mut state, "A-Master");
        let doc = state.active().document();
        assert_eq!(doc.master_of_page(page(&state, 3)), Some(master));
        assert_eq!(doc.master_of_page(page(&state, 2)), None, "not its partner");

        click_with(&ctx, &mut state, "Page 5", egui::Modifiers::SHIFT);
        click(&ctx, &mut state, "A-Master");
        let doc = state.active().document();
        for n in 3..=5 {
            assert_eq!(
                doc.master_of_page(page(&state, n)),
                Some(master),
                "page {n}"
            );
        }
        apply(&mut state, Command::Undo);
        let doc = state.active().document();
        assert_eq!(
            doc.master_of_page(page(&state, 4)),
            None,
            "one step for all three"
        );
        assert_eq!(doc.master_of_page(page(&state, 3)), Some(master));
    }

    #[test]
    fn a_parent_dragged_onto_a_page_is_put_on_it() {
        let mut state = six_pages();
        apply(&mut state, Command::AddMaster);
        let master = state.active().document().master_order[0];
        let ctx = a_panel();
        let from = at(&ctx, &mut state, "A-Master");
        let to = at(&ctx, &mut state, "Page 4");
        panel(&ctx, &mut state, press(from, true, egui::Modifiers::NONE));
        let steps = 6;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            panel(
                &ctx,
                &mut state,
                vec![egui::Event::PointerMoved(from + (to - from) * t)],
            );
        }
        panel(&ctx, &mut state, press(to, false, egui::Modifiers::NONE));
        panel(&ctx, &mut state, Vec::new());
        assert_eq!(
            state.active().document().master_of_page(page(&state, 4)),
            Some(master)
        );
        assert_eq!(
            state.active().document().master_of_page(page(&state, 5)),
            None
        );
    }

    #[test]
    fn insert_puts_a_page_after_the_chosen_one_and_chooses_it() {
        let mut state = six_pages();
        let ctx = a_panel();
        let third = page(&state, 3);
        click(&ctx, &mut state, "Page 3");
        click(&ctx, &mut state, "Insert page");
        let doc = state.active().document();
        assert_eq!(doc.page_ids().count(), 7);
        let new = page(&state, 4);
        assert_eq!(page(&state, 3), third);
        assert!(![third].contains(&new));
        assert_eq!(targets(&state), [new], "the new page is chosen");
        assert_eq!(summary(&state), "Page 4");
    }

    #[test]
    fn insert_pages_puts_as_many_as_asked_where_asked_on_the_parent_asked() {
        let mut state = six_pages();
        apply(&mut state, Command::AddMaster);
        let master = state.active().document().master_order[0];
        let ctx = a_panel();
        click(&ctx, &mut state, "Insert pages\u{2026}");
        assert!(state.pages_window.inserting.is_some(), "the form is open");
        state.pages_window.inserting = Some(InsertForm {
            count: 3,
            at: InsertAt::End,
            parent: InsertParent::Master(master),
        });
        let depth = state.active().history.undo_depth();
        click(&ctx, &mut state, "Insert 3 pages");
        assert!(state.pages_window.inserting.is_none(), "and closes");
        let doc = state.active().document();
        assert_eq!(doc.page_ids().count(), 9);
        let made: Vec<PageId> = doc.page_ids().skip(6).collect();
        assert_eq!(targets(&state), made, "the new pages are chosen");
        assert!(made.iter().all(|p| doc.master_of_page(*p) == Some(master)));
        assert_eq!(state.active().history.undo_depth(), depth + 1, "one step");
    }

    #[test]
    fn chosen_pages_take_a_size_and_turn_as_one_step() {
        use tessera_document::nodes::PagePreset;
        let mut state = six_pages();
        let (two, three) = (page(&state, 2), page(&state, 3));
        state.pages_window.selected = vec![two, three];
        state.pages_window.of = Some(state.active);
        let depth = state.active().history.undo_depth();
        let (w, h) = PagePreset::A5.size();
        run(&mut state, PageAct::Size(w, h));
        assert_eq!(state.active().history.undo_depth(), depth + 1);
        let size = |state: &TesseraApp, p: PageId| {
            let b = state.active().document().pages[p].bounds;
            (b.width, b.height)
        };
        assert_eq!(size(&state, two), (w, h));
        assert_eq!(size(&state, three), (w, h));
        assert_eq!(sizes_said(&state, &[two, three]), "A5");

        run(&mut state, PageAct::Turn);
        assert_eq!(size(&state, two), (h, w), "turned a quarter");
        assert_eq!(sizes_said(&state, &[two, three]), "A5 landscape");
        let first = page(&state, 1);
        assert_eq!(sizes_said(&state, &[first, two]), "sizes differ");
    }

    #[test]
    fn a_size_is_named_by_its_preset_or_measured() {
        use tessera_document::nodes::PagePreset;
        let mm = tessera_geometry::Unit::Millimetres;
        let (w, h) = PagePreset::A4.size();
        assert_eq!(size_name(w, h, mm), "A4");
        let (w, h) = PagePreset::Letter.size();
        assert_eq!(size_name(h, w, mm), "Letter landscape");
        assert_eq!(
            size_name(100.0 * 72.0 / 25.4, 50.0 * 72.0 / 25.4, mm),
            "100 \u{00d7} 50 mm"
        );
    }

    #[test]
    fn duplicate_copies_the_chosen_run_after_itself_and_chooses_the_copies() {
        let mut state = six_pages();
        let ctx = a_panel();
        let (two, three) = (page(&state, 2), page(&state, 3));
        click(&ctx, &mut state, "Page 2");
        click_with(&ctx, &mut state, "Page 3", egui::Modifiers::SHIFT);
        click(&ctx, &mut state, "Duplicate pages");
        let order: Vec<PageId> = state.active().document().page_ids().collect();
        assert_eq!(order.len(), 8);
        assert_eq!(order[1..3], [two, three]);
        assert_eq!(targets(&state), order[3..5], "the copies, chosen");
        assert_eq!(summary(&state), "Pages 4\u{2013}5");
    }

    #[test]
    fn delete_takes_the_chosen_pages_and_is_refused_for_all_of_them() {
        let mut state = six_pages();
        let ctx = a_panel();
        let (one, four) = (page(&state, 1), page(&state, 4));
        click(&ctx, &mut state, "Page 2");
        click_with(&ctx, &mut state, "Page 3", egui::Modifiers::SHIFT);
        click(&ctx, &mut state, "Delete pages");
        let order: Vec<PageId> = state.active().document().page_ids().collect();
        assert_eq!(order.len(), 4);
        assert_eq!(order[..2], [one, four]);

        // Every page chosen: the button does nothing, as the document would
        // refuse it anyway.
        let all: Vec<PageId> = state.active().document().page_ids().collect();
        state.pages_window.selected = all;
        run(&mut state, PageAct::Delete);
        assert_eq!(state.active().document().page_ids().count(), 4);
    }

    #[test]
    fn a_parent_is_renamed_in_place() {
        let mut state = six_pages();
        apply(&mut state, Command::AddMaster);
        let master = state.active().document().master_order[0];
        state.pages_window.renaming = Some((master, "A-Master".into()));
        let ctx = a_panel();
        panel(&ctx, &mut state, Vec::new());
        panel(
            &ctx,
            &mut state,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                },
                egui::Event::Text("A-Chapter".into()),
            ],
        );
        panel(
            &ctx,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert_eq!(state.active().document().masters[master].name, "A-Chapter");
        assert!(state.pages_window.renaming.is_none());
    }

    #[test]
    fn a_parents_letter_is_its_prefix() {
        assert_eq!(prefix("A-Master"), "A");
        assert_eq!(prefix("BK-Back matter"), "BK");
        assert_eq!(prefix("Chapter opener"), "C");
        assert_eq!(prefix("a very-long-prefix name"), "A");
    }

    #[test]
    fn the_numbering_options_open_on_the_page_asked() {
        let mut state = six_pages();
        let four = page(&state, 4);
        run(&mut state, PageAct::Numbering(four));
        assert!(state.numbering.open);
        assert_eq!(state.numbering.page, Some(four));
    }

    #[test]
    fn a_chosen_page_that_is_deleted_elsewhere_is_forgotten() {
        let mut state = six_pages();
        let two = page(&state, 2);
        state.pages_window.selected = vec![two];
        state.pages_window.of = Some(state.active);
        apply(&mut state, Command::RemovePage { id: two });
        tidy(&mut state);
        assert!(state.pages_window.selected.is_empty());
        assert_ne!(targets(&state), [two]);
    }

    #[test]
    fn dragging_one_of_the_chosen_pages_moves_them_all_and_undoes_in_one_step() {
        let mut state = six_pages();
        let ctx = a_panel();
        let before: Vec<PageId> = state.active().document().page_ids().collect();
        click(&ctx, &mut state, "Page 2");
        click_with(&ctx, &mut state, "Page 3", egui::Modifiers::SHIFT);
        let from = at(&ctx, &mut state, "Page 2");
        // The right half of page five: after it.
        let five = at(&ctx, &mut state, "Page 5");
        let to = five + Vec2::new(8.0, 0.0);
        panel(&ctx, &mut state, press(from, true, egui::Modifiers::NONE));
        for i in 1..=6 {
            let t = i as f32 / 6.0;
            panel(
                &ctx,
                &mut state,
                vec![egui::Event::PointerMoved(from + (to - from) * t)],
            );
        }
        panel(&ctx, &mut state, press(to, false, egui::Modifiers::NONE));
        panel(&ctx, &mut state, Vec::new());
        let after: Vec<PageId> = state.active().document().page_ids().collect();
        assert_eq!(
            after,
            [
                before[0], before[3], before[4], before[1], before[2], before[5]
            ],
            "2 and 3 together, after 5"
        );
        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().page_ids().collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn the_document_is_laid_out_as_the_document_while_a_parent_is_open() {
        // What a page's thumbnail is drawn from. Drawn from the canvas's
        // layout, every page went blank at the first change made while a
        // parent was open.
        let mut state = six_pages();
        let first = state.active().document().pages[page(&state, 1)].bounds;
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: first.x + 10.0,
                y: first.y + 10.0,
                width: 20.0,
                height: 20.0,
            }),
        );
        let rectangle = state.active().selection.single().expect("selected");
        apply(&mut state, Command::AddMaster);
        let master = state.active().document().master_order[0];
        state.edit_master(Some(master));

        let on_canvas = |state: &mut TesseraApp| {
            state
                .resolve_active()
                .items
                .iter()
                .any(|item| item.frame == rectangle)
        };
        assert!(!on_canvas(&mut state), "the canvas shows the parent");
        assert!(
            state
                .resolve_in(Scope::Document)
                .items
                .iter()
                .any(|item| item.frame == rectangle),
            "the thumbnails the document"
        );
        assert!(
            !on_canvas(&mut state),
            "and the canvas's layout is left alone"
        );
    }
}
