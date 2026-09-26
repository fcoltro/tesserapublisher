//! The Book panel: the chapters of a publication, in order, and what is
//! done to them together.
//!
//! A book is a file listing documents (see `tessera_document::book`). The
//! panel opens or makes one, adds and orders its chapters, and offers what a
//! book is for: numbering the pages on from chapter to chapter, one contents
//! listing every chapter's headings, one preflight of them all, and one PDF.
//! The work itself is in `book_ops`; this is what is shown and pressed.
//!
//! **It says what state the book is in.** Each chapter's row says the pages
//! it runs to as the book numbers them, whether it is open — and unsaved —
//! in a tab, whether it is missing, and, once checked, how many preflight
//! problems it has; the top says when the chapters' own numbers are not the
//! book's, which is the one thing about a book that goes wrong silently. A
//! list of names said none of it.
//!
//! **Every change to the list is saved as it is made.** The list is the
//! whole book file, and a change kept only in the panel until somebody
//! pressed Save was lost with the window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Rect, Sense, Stroke, Ui, Vec2};
use tessera_document::book::{Book, EXTENSION};

use super::{panel_ui, style_ui};
use crate::app::{Status, TesseraApp};
use crate::book_ops::{ChapterState, Stamp, Summary};
use crate::icons::Icon;
use crate::theme::Theme;

/// A chapter's row: its name, and a line of what is true of it.
const ROW: f32 = 44.0;

/// How many books the panel remembers.
const RECENT_BOOKS: usize = 6;

#[derive(Debug, Clone, Default)]
pub struct BookPanel {
    pub open: bool,
    /// The book file, once one is open or made.
    pub path: Option<PathBuf>,
    pub book: Book,
    /// Every chapter summarised, kept until one changes.
    pub summaries: crate::book_ops::Summaries,
    /// The chapter chosen, by its place in the list.
    pub selected: Option<usize>,
    /// Each chapter's preflight counts, and the stamp of the chapter they
    /// were counted in: shown while the chapter is still that.
    checked: HashMap<PathBuf, (Stamp, (usize, usize))>,
    /// The chapter being dragged, while it is.
    dragging: Option<usize>,
}

impl BookPanel {
    /// The chapters' paths, made absolute against the book file.
    pub fn chapters(&self) -> Vec<PathBuf> {
        match &self.path {
            Some(path) => self.book.resolved(path),
            None => self.book.documents.clone(),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        self.book.save(path)
    }
}

fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// A page range as a folio line says it: "pp. 13–30", or "p. 7".
fn pages_said(range: &(String, String)) -> String {
    if range.0 == range.1 {
        format!("p.\u{2009}{}", range.0)
    } else {
        format!("pp.\u{2009}{}\u{2013}{}", range.0, range.1)
    }
}

fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// What a click in the panel asked for, done once it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Act {
    Select(usize),
    Open(usize),
    Locate(usize),
    Reveal(usize),
    Remove(usize),
    Move { from: usize, to: usize },
    Add,
    AddCurrent,
    Numbering(bool),
    Number,
    Contents,
    Preflight,
    Export,
    NewBook,
    OpenBook,
    OpenRecent(PathBuf),
    CloseBook,
}

pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    let Some(path) = state.book.path.clone() else {
        if let Some(act) = no_book(ui, state) {
            run(state, act);
        }
        return;
    };
    let chapters = state.book.chapters();
    let numbering = state.book.book.continue_numbering;
    let summaries = crate::book_ops::summaries(state, &chapters, numbering);
    let missing = summaries
        .iter()
        .filter(|s| !matches!(s.state, ChapterState::Open { .. } | ChapterState::Closed))
        .count();
    let stale = summaries.iter().filter(|s| s.out_of_date()).count();
    if state.book.selected.is_some_and(|i| i >= chapters.len()) {
        state.book.selected = None;
    }

    let mut act = header(ui, state, &path, &summaries, missing);

    // The one thing about a book that goes wrong without a word: chapters
    // whose own numbers are not the ones the book gives them.
    if numbering && stale > 0 {
        style_ui::card(ui, None, |ui| {
            ui.horizontal(|ui| {
                let (mark, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                crate::icons::paint(ui.painter(), mark, Icon::WarningMark, Theme::accent());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!(
                            "Page numbers are out of date in {}.",
                            count(stale, "chapter")
                        ))
                        .color(Theme::text_primary()),
                    )
                    .wrap(),
                );
            });
            if panel_ui::action_when(ui, missing == 0, Icon::Pages, "Number pages")
                .on_hover_text(if missing == 0 {
                    "Number each chapter on from the one before"
                } else {
                    "Find the missing chapters first"
                })
                .clicked()
            {
                act = Some(Act::Number);
            }
        });
    }

    if chapters.is_empty() {
        panel_ui::empty(
            ui,
            "No chapters yet",
            "Add saved documents, in the order they are read.",
        );
    }
    if let Some(asked) = chapter_list(ui, state, &summaries) {
        act = Some(asked);
    }

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if panel_ui::action(ui, Icon::Plus, "Add chapters\u{2026}")
            .on_hover_text("Choose saved documents to add at the end")
            .clicked()
        {
            act = Some(Act::Add);
        }
        let current = state.active().current_path.clone();
        let listed = current
            .as_ref()
            .is_some_and(|c| summaries.iter().any(|s| same_file(&s.path, c)));
        if current.is_some()
            && !listed
            && panel_ui::action(ui, Icon::Plus, "Add this document")
                .on_hover_text("Add the document open in front, at the end")
                .clicked()
        {
            act = Some(Act::AddCurrent);
        }
    });

    if let Some(index) = state.book.selected
        && let Some(summary) = summaries.get(index)
        && let Some(asked) = chosen_actions(ui, index, summary)
    {
        act = Some(asked);
    }

    ui.add_space(Theme::space_2());
    if let Some(asked) = publish(ui, state, &summaries, missing, stale) {
        act = Some(asked);
    }

    if let Some(act) = act {
        run(state, act);
    }
}

/// Before a book is open: what one is for, the two ways to one, and the
/// books opened lately.
fn no_book(ui: &mut Ui, state: &TesseraApp) -> Option<Act> {
    let mut act = None;
    panel_ui::empty(
        ui,
        "Bring your chapters together",
        "A book lists documents in order, to number their pages on from one to the next, \
         list them in one contents and export them as one PDF.",
    );
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if panel_ui::action(ui, Icon::Plus, "New book\u{2026}").clicked() {
            act = Some(Act::NewBook);
        }
        if panel_ui::action(ui, Icon::Book, "Open book\u{2026}").clicked() {
            act = Some(Act::OpenBook);
        }
    });
    if !state.prefs.recent_books.is_empty() {
        ui.add_space(Theme::space_2());
        style_ui::overline(ui, "Recent books");
        for path in &state.prefs.recent_books {
            let there = path.exists();
            ui.add_enabled_ui(there, |ui| {
                if style_ui::page_link(ui, Icon::Book, &name_of(path), ui.available_width())
                    .on_hover_text(path.display().to_string())
                    .on_disabled_hover_text(format!("Not there any more: {}", path.display()))
                    .clicked()
                {
                    act = Some(Act::OpenRecent(path.clone()));
                }
            });
        }
    }
    act
}

/// The book's name, what it holds, and the book's own menu.
fn header(
    ui: &mut Ui,
    state: &TesseraApp,
    path: &Path,
    summaries: &[Summary],
    missing: usize,
) -> Option<Act> {
    let mut act = None;
    let pages: usize = summaries.iter().map(|s| s.pages).sum();
    style_ui::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
            ui.painter().rect_filled(mark, 6.0, Theme::accent_soft());
            crate::icons::paint(ui.painter(), mark, Icon::Book, Theme::accent());
            // The words take what the menu leaves, and no more: a column
            // as wide as the row, with the menu beside it, overflowed the
            // dock, and a dock that fits its contents grew every frame.
            let room = (ui.available_width() - 40.0).max(40.0);
            ui.vertical(|ui| {
                ui.set_max_width(room);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(name_of(path))
                            .font(style_ui::heading_font(15.0))
                            .color(Theme::text_primary()),
                    )
                    .truncate(),
                )
                .on_hover_text(path.display().to_string());
                let mut said = format!(
                    "{} \u{00b7} {}",
                    count(summaries.len(), "chapter"),
                    count(pages, "page")
                );
                if missing > 0 {
                    said.push_str(&format!(" \u{00b7} {missing} missing"));
                }
                ui.label(
                    egui::RichText::new(said)
                        .size(Theme::TYPE_SM)
                        .color(if missing > 0 {
                            Theme::error()
                        } else {
                            Theme::text_muted()
                        }),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let menu = ui.menu_button(egui::RichText::new("\u{2022}\u{2022}\u{2022}"), |ui| {
                    if ui.button("New book\u{2026}").clicked() {
                        act = Some(Act::NewBook);
                        ui.close();
                    }
                    if ui.button("Open book\u{2026}").clicked() {
                        act = Some(Act::OpenBook);
                        ui.close();
                    }
                    let others: Vec<&PathBuf> = state
                        .prefs
                        .recent_books
                        .iter()
                        .filter(|p| p.as_path() != path)
                        .collect();
                    if !others.is_empty() {
                        ui.menu_button("Open recent", |ui| {
                            for other in others {
                                if ui
                                    .add_enabled(other.exists(), egui::Button::new(name_of(other)))
                                    .on_hover_text(other.display().to_string())
                                    .clicked()
                                {
                                    act = Some(Act::OpenRecent(other.clone()));
                                    ui.close();
                                }
                            }
                        });
                    }
                    ui.separator();
                    if ui.button(super::links::file_manager()).clicked() {
                        crate::view::links::reveal_in_file_manager(path);
                        ui.close();
                    }
                    if ui.button("Close book").clicked() {
                        act = Some(Act::CloseBook);
                        ui.close();
                    }
                });
                crate::icons::named(menu.response, "Book menu");
            });
        });
    });
    act
}

/// Every chapter, in order: dragged to reorder, clicked to choose,
/// double-clicked to open.
fn chapter_list(ui: &mut Ui, state: &mut TesseraApp, summaries: &[Summary]) -> Option<Act> {
    let mut act = None;
    let mut rects: Vec<Rect> = Vec::new();
    let mut released = false;
    let mut pointer = None;
    ui.spacing_mut().item_spacing.y = 2.0;
    for (index, summary) in summaries.iter().enumerate() {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), ROW),
            Sense::click_and_drag(),
        );
        rects.push(rect);
        chapter_row(ui, state, index, summary, rect, &response);
        let name = name_of(&summary.path);
        let response = crate::icons::reads_as(
            response,
            &name,
            egui::WidgetType::SelectableLabel,
            Some(state.book.selected == Some(index)),
        )
        .on_hover_text(summary.path.display().to_string());
        if response.drag_started() {
            state.book.dragging = Some(index);
        }
        if response.dragged() || response.drag_stopped() {
            pointer = response.interact_pointer_pos().or(pointer);
        }
        if response.drag_stopped() {
            released = true;
        }
        if response.double_clicked() && summary_openable(summary) {
            act = Some(Act::Open(index));
        } else if response.clicked() {
            act = Some(Act::Select(index));
        }
        let last = summaries.len() - 1;
        response.context_menu(|ui| {
            let mut item = |ui: &mut Ui, enabled: bool, label: &str, asked: Act| {
                if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                    act = Some(asked);
                    ui.close();
                }
            };
            let openable = summary_openable(summary);
            item(ui, openable, "Open chapter", Act::Open(index));
            item(ui, true, "Locate\u{2026}", Act::Locate(index));
            item(
                ui,
                openable,
                super::links::file_manager(),
                Act::Reveal(index),
            );
            ui.separator();
            item(
                ui,
                index > 0,
                "Move earlier",
                Act::Move {
                    from: index,
                    to: index.saturating_sub(1),
                },
            );
            item(
                ui,
                index < last,
                "Move later",
                Act::Move {
                    from: index,
                    to: index + 2,
                },
            );
            ui.separator();
            item(ui, true, "Remove from book", Act::Remove(index));
        });
    }

    // While a chapter is dragged: a line where it would land.
    if let (Some(from), Some(at)) = (state.book.dragging, pointer) {
        let slot = rects
            .iter()
            .position(|r| at.y < r.center().y)
            .unwrap_or(rects.len());
        if released {
            state.book.dragging = None;
            act = Some(Act::Move { from, to: slot });
        } else if let Some(y) = rects
            .get(slot)
            .map(|r| r.top() - 1.0)
            .or_else(|| rects.last().map(|r| r.bottom() + 1.0))
        {
            let left = rects[0].left();
            let right = rects[0].right();
            ui.painter().line_segment(
                [egui::pos2(left + 4.0, y), egui::pos2(right - 4.0, y)],
                Stroke::new(2.0, Theme::accent()),
            );
        }
    } else if released {
        state.book.dragging = None;
    }
    act
}

fn summary_openable(summary: &Summary) -> bool {
    matches!(
        summary.state,
        ChapterState::Open { .. } | ChapterState::Closed
    )
}

/// One chapter: its place, its name, the pages the book gives it, and what
/// state it is in.
fn chapter_row(
    ui: &Ui,
    state: &TesseraApp,
    index: usize,
    summary: &Summary,
    rect: Rect,
    response: &egui::Response,
) {
    let painter = ui.painter_at(rect);
    let chosen = state.book.selected == Some(index);
    let dragged = state.book.dragging == Some(index) && response.dragged();
    if chosen || dragged {
        painter.rect_filled(rect, 6.0, Theme::accent_soft());
    } else if response.hovered() {
        painter.rect_filled(rect, 6.0, Theme::hover_bg());
    }
    let broken = !summary_openable(summary);

    // Its place in the book, in a disc.
    let disc = egui::pos2(rect.left() + 16.0, rect.center().y);
    painter.circle_filled(
        disc,
        11.0,
        if broken {
            Theme::error().gamma_multiply(0.18)
        } else {
            style_ui::well_fill()
        },
    );
    painter.text(
        disc,
        egui::Align2::CENTER_CENTER,
        (index + 1).to_string(),
        egui::FontId::proportional(Theme::TYPE_SM),
        if broken {
            Theme::error()
        } else {
            Theme::text_muted()
        },
    );

    let left = rect.left() + 36.0;
    let mut right = rect.right() - 8.0;
    let small = egui::FontId::proportional(Theme::TYPE_SM);

    // At the right: open in a tab, and what a check of it found.
    if let Some((stamp, (errors, warnings))) = state.book.checked.get(&summary.path)
        && *stamp == summary.stamp
    {
        let (icon, tint, n) = if *errors > 0 {
            (Icon::ErrorMark, Theme::error(), *errors)
        } else if *warnings > 0 {
            (Icon::WarningMark, Theme::accent(), *warnings)
        } else {
            (Icon::Preflight, Theme::text_muted(), 0)
        };
        let text = if n > 0 { n.to_string() } else { String::new() };
        let galley = painter.layout_no_wrap(text, small.clone(), tint);
        let width = 16.0 + if n > 0 { galley.size().x + 3.0 } else { 0.0 };
        let at = egui::pos2(right - width, rect.top() + 8.0);
        crate::icons::paint(
            &painter,
            Rect::from_min_size(at, Vec2::splat(14.0)),
            icon,
            tint,
        );
        painter.galley(egui::pos2(at.x + 17.0, at.y), galley, tint);
        right -= width + 6.0;
    }
    if let ChapterState::Open { unsaved } = summary.state {
        let dot = egui::pos2(right - 4.0, rect.top() + 15.0);
        if unsaved {
            painter.circle_filled(dot, 4.0, Theme::accent());
        } else {
            painter.circle_stroke(dot, 3.5, Stroke::new(1.5, Theme::accent()));
        }
        right -= 14.0;
    }

    let mut job = egui::text::LayoutJob::simple_singleline(
        name_of(&summary.path),
        egui::TextStyle::Body.resolve(ui.style()),
        if broken {
            Theme::error()
        } else {
            Theme::text_primary()
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(12.0));
    painter.galley(
        egui::pos2(left, rect.top() + 5.0),
        painter.layout_job(job),
        Theme::text_primary(),
    );

    // Under it: the pages it runs to in the book, or what is wrong.
    let mut job = egui::text::LayoutJob::default();
    let muted = egui::TextFormat::simple(small.clone(), Theme::text_muted());
    match &summary.state {
        ChapterState::Missing => job.append(
            "Missing: not where the book says",
            0.0,
            egui::TextFormat::simple(small.clone(), Theme::error()),
        ),
        ChapterState::Unreadable(_) => job.append(
            "Not a document this can read",
            0.0,
            egui::TextFormat::simple(small.clone(), Theme::error()),
        ),
        ChapterState::Open { .. } | ChapterState::Closed => {
            let range = summary.in_book.as_ref().map(pages_said).unwrap_or_default();
            job.append(
                &format!("{range} \u{00b7} {}", count(summary.pages, "page")),
                0.0,
                muted.clone(),
            );
            if summary.out_of_date() && state.book.book.continue_numbering {
                job.append(
                    " \u{00b7} numbers out of date",
                    0.0,
                    egui::TextFormat::simple(small.clone(), Theme::accent()),
                );
            }
            if let ChapterState::Open { unsaved } = summary.state {
                job.append(
                    if unsaved {
                        " \u{00b7} open, unsaved"
                    } else {
                        " \u{00b7} open"
                    },
                    0.0,
                    muted,
                );
            }
        }
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - 8.0 - left).max(12.0));
    let line = painter.layout_job(job);
    painter.galley(
        egui::pos2(left, rect.bottom() - 6.0 - line.size().y),
        line,
        Theme::text_muted(),
    );
}

/// What can be done with the chapter chosen.
fn chosen_actions(ui: &mut Ui, index: usize, summary: &Summary) -> Option<Act> {
    let mut act = None;
    ui.add_space(4.0);
    style_ui::card(ui, None, |ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(summary.path.display().to_string())
                    .size(Theme::TYPE_SM)
                    .color(Theme::text_muted()),
            )
            .truncate(),
        )
        .on_hover_text(summary.path.display().to_string());
        ui.horizontal_wrapped(|ui| {
            if summary_openable(summary) {
                if panel_ui::action(ui, Icon::Pages, "Open")
                    .on_hover_text("Open the chapter in a tab")
                    .clicked()
                {
                    act = Some(Act::Open(index));
                }
            } else if panel_ui::action(ui, Icon::Link2, "Locate\u{2026}")
                .on_hover_text("Choose where the chapter is now")
                .clicked()
            {
                act = Some(Act::Locate(index));
            }
            if panel_ui::action(ui, Icon::Trash, "Remove")
                .on_hover_text("Take it out of the book. The file stays where it is.")
                .clicked()
            {
                act = Some(Act::Remove(index));
            }
        });
    });
    act
}

/// What the book is for, together: numbering, contents, a check, one PDF.
fn publish(
    ui: &mut Ui,
    state: &TesseraApp,
    summaries: &[Summary],
    missing: usize,
    stale: usize,
) -> Option<Act> {
    let mut act = None;
    let any = !summaries.is_empty();
    style_ui::card(ui, Some("Publish"), |ui| {
        let mut numbering = state.book.book.continue_numbering;
        if ui
            .checkbox(&mut numbering, "Number pages on from chapter to chapter")
            .on_hover_text("Each chapter's first page takes the number after the one before's last")
            .changed()
        {
            act = Some(Act::Numbering(numbering));
        }
        ui.add_space(4.0);
        let active = state.active().current_path.clone();
        let chapter_in_front = active
            .as_ref()
            .is_some_and(|a| summaries.iter().any(|s| same_file(&s.path, a)));
        ui.horizontal_wrapped(|ui| {
            let can = any && numbering && missing == 0;
            if panel_ui::action_when(ui, can, Icon::Pages, "Number pages")
                .on_hover_text(if missing > 0 {
                    "Find the missing chapters first: a book numbered around a gap is \
                     numbered wrong"
                } else if !numbering {
                    "Switch numbering on from chapter to chapter first"
                } else if stale > 0 {
                    "Number each chapter on from the one before: open chapters as a \
                     change to undo, the rest saved back to their files"
                } else {
                    "Every chapter is numbered as the book numbers it already"
                })
                .clicked()
            {
                act = Some(Act::Number);
            }
            let can = any && chapter_in_front && missing == 0;
            if panel_ui::action_when(ui, can, Icon::List, "Update contents")
                .on_hover_text(if can {
                    "Rebuild the contents in the chapter in front from every chapter's \
                     headings"
                } else {
                    "Open the chapter that holds the contents: they are built where its \
                     recipe is"
                })
                .clicked()
            {
                act = Some(Act::Contents);
            }
            if panel_ui::action_when(ui, any, Icon::Preflight, "Check chapters")
                .on_hover_text("Preflight every chapter, and show what each has")
                .clicked()
            {
                act = Some(Act::Preflight);
            }
            let can = any && missing == 0;
            if panel_ui::action_when(ui, can, Icon::Duplicate, "Export PDF\u{2026}")
                .on_hover_text(if can {
                    "Every chapter, in order, as one PDF"
                } else {
                    "Find the missing chapters first"
                })
                .clicked()
            {
                act = Some(Act::Export);
            }
        });
    });
    act
}

/// Whether two paths name one file, allowing for the spellings a path can
/// have on the way through a dialog and a book file.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || match (a.canonicalize(), b.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

fn run(state: &mut TesseraApp, act: Act) {
    let chapters = state.book.chapters();
    match act {
        Act::Select(index) => state.book.selected = Some(index),
        Act::Open(index) => {
            state.book.selected = Some(index);
            if let Some(path) = chapters.get(index) {
                open_chapter(state, path);
            }
        }
        Act::Locate(index) => {
            let Some(book_path) = state.book.path.clone() else {
                return;
            };
            if let Some(found) = rfd::FileDialog::new()
                .add_filter("Tessera document", &[crate::file_ops::EXTENSION])
                .pick_file()
            {
                if state.book.book.locate(&book_path, index, &found) {
                    changed(state);
                } else {
                    state.status = Some(Status::error("that document is in the book already"));
                }
            }
        }
        Act::Reveal(index) => {
            if let Some(path) = chapters.get(index) {
                crate::view::links::reveal_in_file_manager(path);
            }
        }
        Act::Remove(index) => {
            if state.book.book.remove(index) {
                state.book.selected = None;
                changed(state);
            }
        }
        Act::Move { from, to } => {
            if state.book.book.move_to(from, to) {
                state.book.selected = Some(if to > from { to - 1 } else { to });
                changed(state);
            }
        }
        Act::Add => add_documents(state),
        Act::AddCurrent => {
            if let (Some(book_path), Some(current)) =
                (state.book.path.clone(), state.active().current_path.clone())
            {
                state.book.book.add(&book_path, &current);
                changed(state);
            }
        }
        Act::Numbering(on) => {
            state.book.book.continue_numbering = on;
            changed(state);
        }
        Act::Number => match crate::book_ops::continue_numbering(state, &chapters) {
            Ok(0) => state.status = Some(Status::info("every chapter is numbered already")),
            Ok(n) => {
                state.status = Some(Status::info(format!("{} renumbered", count(n, "chapter"))));
            }
            Err(e) => state.status = Some(Status::error(format!("could not number: {e}"))),
        },
        Act::Contents => {
            let numbering = state.book.book.continue_numbering;
            match crate::book_ops::update_contents(state, &chapters, numbering) {
                Ok(true) => state.status = Some(Status::info("contents updated across the book")),
                Ok(false) => {
                    state.status = Some(Status::error(
                        "open one of the book's chapters first: the contents go where its recipe is",
                    ));
                }
                Err(e) => state.status = Some(Status::error(format!("could not build: {e}"))),
            }
        }
        Act::Preflight => {
            let found = crate::book_ops::preflight(state, &chapters);
            let summaries =
                crate::book_ops::summaries(state, &chapters, state.book.book.continue_numbering);
            let (mut errors, mut warnings) = (0, 0);
            for (summary, counts) in summaries.iter().zip(found) {
                match counts {
                    Some(counts) => {
                        errors += counts.0;
                        warnings += counts.1;
                        state
                            .book
                            .checked
                            .insert(summary.path.clone(), (summary.stamp.clone(), counts));
                    }
                    None => {
                        state.book.checked.remove(&summary.path);
                    }
                }
            }
            state.status = Some(Status::info(match (errors, warnings) {
                (0, 0) => "no problems in any chapter".to_string(),
                (e, w) => format!(
                    "the chapters have {} and {}",
                    count(e, "error"),
                    count(w, "warning")
                ),
            }));
        }
        Act::Export => {
            let numbering = state.book.book.continue_numbering;
            export_pdf(state, &chapters, numbering);
        }
        Act::NewBook => new_book(state),
        Act::OpenBook => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Tessera book", &[EXTENSION])
                .pick_file()
            {
                open_book(state, &path);
            }
        }
        Act::OpenRecent(path) => open_book(state, &path),
        Act::CloseBook => {
            state.book.path = None;
            state.book.book = Book::default();
            state.book.selected = None;
            state.book.checked.clear();
        }
    }
}

/// Save the list as it now is: every change to it is kept as it is made.
fn changed(state: &mut TesseraApp) {
    if let Err(e) = state.book.save() {
        state.status = Some(Status::error(format!("could not save the book: {e}")));
    }
}

fn open_chapter(state: &mut TesseraApp, path: &Path) {
    if let Err(e) = crate::file_ops::open_from_path(state, path) {
        state.status = Some(Status::error(format!(
            "could not open {}: {e}",
            path.display()
        )));
    }
}

/// Put `path` at the head of the books opened lately, once.
fn remember(state: &mut TesseraApp, path: &Path) {
    let recent = &mut state.prefs.recent_books;
    recent.retain(|p| p != path);
    recent.insert(0, path.to_path_buf());
    recent.truncate(RECENT_BOOKS);
}

fn new_book(state: &mut TesseraApp) {
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter("Tessera book", &[EXTENSION])
        .set_file_name(format!("Untitled.{EXTENSION}"))
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(EXTENSION);
    }
    let book = Book::default();
    if let Err(e) = book.save(&path) {
        state.status = Some(Status::error(format!("could not make the book: {e}")));
        return;
    }
    remember(state, &path);
    state.book.path = Some(path);
    state.book.book = book;
    state.book.selected = None;
    state.book.checked.clear();
}

pub(crate) fn open_book(state: &mut TesseraApp, path: &Path) {
    match Book::load(path) {
        Ok(book) => {
            remember(state, path);
            state.book.path = Some(path.to_path_buf());
            state.book.book = book;
            state.book.selected = None;
            state.book.checked.clear();
        }
        Err(e) => state.status = Some(Status::error(format!("could not open the book: {e}"))),
    }
}

fn add_documents(state: &mut TesseraApp) {
    let Some(book_path) = state.book.path.clone() else {
        return;
    };
    let picked = rfd::FileDialog::new()
        .add_filter("Tessera document", &[crate::file_ops::EXTENSION])
        .pick_files()
        .unwrap_or_default();
    if picked.is_empty() {
        return;
    }
    for path in picked {
        state.book.book.add(&book_path, &path);
    }
    changed(state);
}

fn export_pdf(state: &mut TesseraApp, chapters: &[PathBuf], numbering: bool) {
    let suggested = state
        .book
        .path
        .as_ref()
        .map(|p| p.with_extension("pdf"))
        .unwrap_or_else(|| PathBuf::from("Book.pdf"));
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter("PDF", &["pdf"])
        .set_file_name(
            suggested
                .file_name()
                .map_or_else(|| "Book.pdf".to_string(), |n| n.to_string_lossy().into()),
        )
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension("pdf");
    }
    let resolved = match crate::book_ops::resolve_book(state, chapters, numbering) {
        Ok(resolved) => resolved,
        Err(e) => {
            state.status = Some(Status::error(format!("could not read a chapter: {e}")));
            return;
        }
    };
    let options = state.export.options(state);
    let result = tessera_pdf::export_with(&resolved, &options)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            tessera_io::atomic::write_atomic(&path, &bytes).map_err(|e| e.to_string())
        });
    state.status = Some(match result {
        Ok(()) => Status::info(format!("Exported the book to {}", path.display())),
        Err(e) => Status::error(format!("could not export the book: {e}")),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
                    egui::vec2(300.0, 1000.0),
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

    fn a_panel() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    fn find(ctx: &egui::Context, state: &mut TesseraApp, label: &str) -> egui::Rect {
        panel(ctx, state, Vec::new());
        let nodes = panel(ctx, state, Vec::new());
        nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
    }

    fn press(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        let at = find(ctx, state, label).center();
        for pressed in [true, false] {
            panel(ctx, state, press(at, pressed));
        }
    }

    fn drag(ctx: &egui::Context, state: &mut TesseraApp, from: egui::Pos2, to: egui::Pos2) {
        panel(ctx, state, press(from, true));
        for i in 1..=6 {
            let t = i as f32 / 6.0;
            panel(
                ctx,
                state,
                vec![egui::Event::PointerMoved(from + (to - from) * t)],
            );
        }
        panel(ctx, state, press(to, false));
        panel(ctx, state, Vec::new());
    }

    /// A document of `pages` pages, saved at `folder/name.tsrdf`.
    fn chapter(folder: &Path, name: &str, pages: usize) -> PathBuf {
        let mut doc = tessera_document::document::Document::new();
        for _ in 1..pages {
            doc.add_page();
        }
        let path = folder.join(format!("{name}.tsrdf"));
        tessera_document::format::save(&doc, &path).unwrap();
        path
    }

    /// A book of three chapters, saved, and open in the panel.
    fn a_book(name: &str) -> (TesseraApp, PathBuf, PathBuf) {
        let folder =
            std::env::temp_dir().join(format!("tessera-book-panel-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let book_path = folder.join("Novel.tesserabook");
        let mut book = Book::default();
        for (chapter_name, pages) in [("One", 3), ("Two", 2), ("Three", 1)] {
            book.add(&book_path, &chapter(&folder, chapter_name, pages));
        }
        book.save(&book_path).unwrap();
        let mut state = TesseraApp::headless();
        open_book(&mut state, &book_path);
        (state, book_path, folder)
    }

    fn order(book: &Book) -> Vec<String> {
        book.documents.iter().map(|p| name_of(p)).collect()
    }

    #[test]
    fn with_no_book_it_offers_one_and_the_books_opened_lately() {
        let (state, book_path, folder) = a_book("recent");
        assert_eq!(
            state.prefs.recent_books,
            vec![book_path.clone()],
            "remembered"
        );
        let mut fresh = TesseraApp::headless();
        fresh.prefs.recent_books = state.prefs.recent_books.clone();
        let ctx = a_panel();
        find(&ctx, &mut fresh, "New book\u{2026}");
        click(&ctx, &mut fresh, "Novel");
        assert_eq!(fresh.book.path.as_deref(), Some(book_path.as_path()));
        assert_eq!(fresh.book.book.documents.len(), 3);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_chapter_dragged_to_the_end_is_moved_and_the_book_saved() {
        let (mut state, book_path, folder) = a_book("drag");
        let ctx = a_panel();
        let one = find(&ctx, &mut state, "One");
        let three = find(&ctx, &mut state, "Three");
        drag(
            &ctx,
            &mut state,
            one.center(),
            egui::pos2(three.center().x, three.bottom() - 2.0),
        );
        assert_eq!(order(&state.book.book), ["Two", "Three", "One"]);
        assert_eq!(
            order(&Book::load(&book_path).unwrap()),
            ["Two", "Three", "One"],
            "saved as it was made, with no Save to forget"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_chapter_chosen_can_be_taken_out_leaving_its_file() {
        let (mut state, book_path, folder) = a_book("remove");
        let ctx = a_panel();
        click(&ctx, &mut state, "Two");
        assert_eq!(state.book.selected, Some(1));
        click(&ctx, &mut state, "Remove");
        assert_eq!(order(&state.book.book), ["One", "Three"]);
        assert_eq!(Book::load(&book_path).unwrap().documents.len(), 2);
        assert!(folder.join("Two.tsrdf").exists(), "the file stays");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn chapters_out_of_step_are_said_and_numbered_on_with_a_click() {
        let (mut state, _, folder) = a_book("number");
        let ctx = a_panel();
        let chapters = state.book.chapters();
        let before = crate::book_ops::summaries(&mut state, &chapters, true);
        assert_eq!(before.iter().filter(|s| s.out_of_date()).count(), 2);
        assert_eq!(before[1].in_book, Some(("4".into(), "5".into())));
        click(&ctx, &mut state, "Number pages");
        let after = crate::book_ops::summaries(&mut state, &chapters, true);
        assert!(after.iter().all(|s| !s.out_of_date()), "{after:#?}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_missing_chapter_holds_back_numbering_and_can_be_located() {
        let (mut state, book_path, folder) = a_book("missing");
        std::fs::remove_file(folder.join("Two.tsrdf")).unwrap();
        tessera_io::seen::look_now(&folder.join("Two.tsrdf"));
        let ctx = a_panel();
        let chapters = state.book.chapters();
        let summaries = crate::book_ops::summaries(&mut state, &chapters, true);
        assert_eq!(summaries[1].state, ChapterState::Missing);
        let files = |folder: &Path| {
            std::fs::read_dir(folder)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect::<Vec<_>>()
        };
        let before = files(&folder);
        state.status = None;
        click(&ctx, &mut state, "Number pages");
        assert_eq!(files(&folder), before, "nothing numbered around the gap");
        assert!(
            state.status.is_none(),
            "the button is not pressed, rather than pressed and failing: {:?}",
            state.status
        );

        // Found again somewhere else: the entry points at it, saved.
        let moved = chapter(&folder, "Two, moved", 2);
        assert!(state.book.book.locate(&book_path, 1, &moved));
        let chapters = state.book.chapters();
        let summaries = crate::book_ops::summaries(&mut state, &chapters, true);
        assert_eq!(summaries[1].state, ChapterState::Closed);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn checking_the_chapters_counts_each_one_s_problems_until_it_changes() {
        let (mut state, _, folder) = a_book("check");
        let ctx = a_panel();
        click(&ctx, &mut state, "Check chapters");
        let chapters = state.book.chapters();
        assert_eq!(state.book.checked.len(), 3);
        let (stamp, counts) = state.book.checked[&chapters[0]].clone();
        assert_eq!(counts, (0, 1), "no press chosen: one warning");
        let summaries = crate::book_ops::summaries(&mut state, &chapters, true);
        assert_eq!(stamp, summaries[0].stamp, "about the chapter as it is");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn switching_numbering_off_is_saved_to_the_book() {
        let (mut state, book_path, folder) = a_book("switch");
        let ctx = a_panel();
        click(&ctx, &mut state, "Number pages on from chapter to chapter");
        assert!(!state.book.book.continue_numbering);
        assert!(!Book::load(&book_path).unwrap().continue_numbering);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
