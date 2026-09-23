//! The Links panel: every file the document points at, and what the disk
//! says about each.
//!
//! What InDesign's Links panel is for, and what this one is for: a job goes
//! to a printer with its artwork, and the panel is where somebody sees, before
//! it goes, that one picture is a week old and another is not there at all.
//! One row per file rather than per frame — the same photograph placed four
//! times is one thing to relink — with how many frames show it beside it.

use std::time::{Duration, Instant};

use egui::Ui;

use tessera_document::ids::{FrameId, LinkId};
use tessera_document::links::Status;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// How long a status is believed before the disk is asked again.
///
/// Asking is a `metadata` call per link per frame otherwise, and a panel
/// that stats forty files sixty times a second is a panel that makes the
/// whole application stutter on a network drive.
const STATUS_TTL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default)]
pub struct LinksPanel {
    pub open: bool,
    /// The row chosen, whose details show below the list.
    pub selected: Option<LinkId>,
    /// What the disk said, and when it was asked.
    statuses: Vec<(LinkId, Status)>,
    asked: Option<Instant>,
    /// The document revision the statuses were read against.
    revision: u64,
}

impl LinksPanel {
    /// Every link's status, from the cache while it is fresh.
    fn statuses(
        &mut self,
        state_doc: &tessera_document::document::Document,
    ) -> Vec<(LinkId, Status)> {
        let stale = self.asked.is_none_or(|at| at.elapsed() > STATUS_TTL)
            || self.revision != state_doc.revision();
        if self.asked.is_none() {
            // Asked for by a person: the disk itself, not what it last said.
            for link in state_doc.links.values() {
                tessera_io::seen::look_now(&link.path);
            }
        }
        if stale {
            self.statuses = state_doc
                .links
                .iter()
                .map(|(id, link)| (id, link.status()))
                .collect();
            self.asked = Some(Instant::now());
            self.revision = state_doc.revision();
        }
        self.statuses.clone()
    }

    /// Forget what the disk said, so the next frame asks again.
    pub fn recheck(&mut self) {
        self.asked = None;
    }
}

/// One row of the list, as it is drawn.
struct Row {
    link: LinkId,
    name: String,
    status: Status,
    /// The page the first frame showing it is on, as the folio reads.
    page: Option<String>,
    uses: usize,
}

fn rows(state: &mut TesseraApp) -> Vec<Row> {
    let TesseraApp {
        links,
        documents,
        active,
        ..
    } = state;
    let doc = documents[*active].document();
    let statuses = links.statuses(doc);
    let mut rows: Vec<Row> = statuses
        .into_iter()
        .filter_map(|(link, status)| {
            let l = doc.links.get(link)?;
            let frames = doc.frames_using(link);
            let page = frames
                .first()
                .and_then(|f| doc.page_of_frame(*f))
                .and_then(|p| doc.page_label(p));
            Some(Row {
                link,
                name: l
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| l.path.to_string_lossy().into_owned()),
                status,
                page,
                uses: frames.len(),
            })
        })
        .collect();
    // By name, so a row can be found; the panel is a list to look a file up
    // in, not a history of what was placed when.
    rows.sort_by_key(|r| r.name.to_lowercase());
    rows
}

/// The word and the colour a status is shown in.
fn describe(status: Status) -> (&'static str, egui::Color32, Icon) {
    match status {
        Status::Fine => ("Up to date", Theme::text_muted(), Icon::Link2),
        Status::Modified => ("Modified on disk", Theme::accent(), Icon::Link2),
        Status::Missing => ("Missing", Theme::error(), Icon::Unlink2),
    }
}

/// Select the first frame showing a link and bring it into view.
fn go_to(state: &mut TesseraApp, link: LinkId) {
    let Some(frame) = state
        .active()
        .document()
        .frames_using(link)
        .first()
        .copied()
    else {
        return;
    };
    go_to_frame(state, frame);
}

fn go_to_frame(state: &mut TesseraApp, frame: FrameId) {
    state.active_mut().selection.set(frame);
    state.reveal = Some(frame);
}

pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    let rows = rows(state);
    if rows.is_empty() {
        super::panel_ui::empty(
            ui,
            "No linked artwork",
            "Place an image and it is listed here, with whether the file on disk is still the one you placed.",
        );
        return;
    }

    // A file that is not where it was, or not what it was, is the reason the
    // panel exists; said once at the top, so the count is not something to
    // scroll for.
    let missing = rows.iter().filter(|r| r.status == Status::Missing).count();
    let modified = rows.iter().filter(|r| r.status == Status::Modified).count();
    if missing + modified > 0 {
        let mut parts = Vec::new();
        if missing > 0 {
            parts.push(format!("{missing} missing"));
        }
        if modified > 0 {
            parts.push(format!("{modified} modified"));
        }
        ui.colored_label(
            if missing > 0 {
                Theme::error()
            } else {
                Theme::accent()
            },
            parts.join(", "),
        );
    }

    let selected = state.links.selected;
    let mut chosen: Option<LinkId> = None;
    let mut went: Option<LinkId> = None;
    for row in &rows {
        let (word, colour, icon) = describe(row.status);
        let response = ui
            .scope(|ui| {
                ui.spacing_mut().item_spacing.x = Theme::space_1();
                // Room on the right for the mark, the page and the count.
                let reserved = Theme::row() * 0.6 + 4.0 * Theme::space_2() + 56.0;
                let response =
                    super::panel_ui::named_row(ui, selected == Some(row.link), &row.name, reserved);
                // The status mark, the page and the count sit on the row's
                // right, drawn over the entry rather than laid out beside it,
                // so the name keeps the whole width to be read in.
                let rect = response.rect;
                let painter = ui.painter();
                let mut x = rect.right() - Theme::space_2();
                let font = egui::TextStyle::Small.resolve(ui.style());
                let mut trailing = |text: &str, colour: egui::Color32| {
                    let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), colour);
                    x -= galley.size().x;
                    painter.galley(
                        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
                        galley,
                        colour,
                    );
                    x -= Theme::space_2();
                };
                if row.uses != 1 {
                    trailing(&format!("×{}", row.uses), Theme::text_muted());
                }
                if let Some(page) = &row.page {
                    trailing(page, Theme::text_muted());
                }
                let size = Theme::row() * 0.6;
                x -= size;
                crate::icons::paint(
                    painter,
                    egui::Rect::from_center_size(
                        egui::pos2(x + size / 2.0, rect.center().y),
                        egui::vec2(size, size),
                    ),
                    icon,
                    colour,
                );
                response.on_hover_text(word)
            })
            .inner;
        if response.clicked() {
            chosen = Some(row.link);
        }
        if response.double_clicked() {
            went = Some(row.link);
        }
    }
    if let Some(link) = chosen {
        state.links.selected = Some(link);
        go_to(state, link);
    }
    if let Some(link) = went {
        go_to(state, link);
    }

    // The chosen file, in full.
    let Some(link) = state.links.selected else {
        return;
    };
    let Some(row) = rows.iter().find(|r| r.link == link) else {
        state.links.selected = None;
        return;
    };
    let Some(file) = state.active().document().links.get(link).cloned() else {
        return;
    };
    ui.add_space(Theme::space_2());
    ui.separator();
    let (word, colour, _) = describe(row.status);
    ui.colored_label(colour, word);
    ui.add(egui::Label::new(egui::RichText::new(file.path.to_string_lossy()).small()).wrap())
        .on_hover_text("The path the document holds");
    let (w, h) = file.natural;
    if w > 0.0 && h > 0.0 {
        ui.colored_label(
            Theme::text_muted(),
            if tessera_render::images::is_svg(&file.path) {
                format!("{w:.0} × {h:.0} pt")
            } else {
                format!("{w:.0} × {h:.0} px")
            },
        );
    }

    ui.add_space(Theme::space_1());
    ui.horizontal_wrapped(|ui| {
        if super::panel_ui::action(ui, Icon::PlaceImage, "Relink…")
            .on_hover_text("Choose the file this should point at. Every frame showing it follows.")
            .clicked()
            && let Some(path) = crate::file_ops::pick_artwork()
        {
            apply(state, Command::Relink { link, path });
            state.links.recheck();
        }
        if ui
            .add_enabled(row.status == Status::Modified, egui::Button::new("Update"))
            .on_hover_text("Read the changed file again")
            .clicked()
        {
            apply(state, Command::UpdateLink { link });
            state.links.recheck();
        }
        if ui
            .add_enabled(row.uses > 0, egui::Button::new("Go to link"))
            .clicked()
        {
            go_to(state, link);
        }
        if ui
            .add_enabled(
                row.status != Status::Missing,
                egui::Button::new("Reveal in Explorer"),
            )
            .clicked()
        {
            reveal_in_file_manager(&file.path);
        }
    });
}

/// Open the file's folder with the file chosen, the way every file manager
/// can. Best effort: a manager that cannot is a link that is still fine.
fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(dir) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_reads_as_missing_and_a_present_one_as_fine() {
        let (word, _, icon) = describe(Status::Missing);
        assert_eq!(word, "Missing");
        assert_eq!(icon, Icon::Unlink2, "the broken link, not the whole one");
        assert_eq!(describe(Status::Fine).0, "Up to date");
    }

    /// A picture box on the first page, showing `link`.
    fn showing(state: &mut TesseraApp, link: LinkId) -> FrameId {
        let mut b = state.first_page_bounds();
        b.width = 50.0;
        b.height = 50.0;
        apply(state, Command::AddGraphicFrame(b));
        let id = state.active().selection.single().expect("selected");
        state.active_mut().document_mut().place(
            id,
            link,
            tessera_document::graphic::Fit::Proportionally,
        );
        id
    }

    fn linked(state: &mut TesseraApp, path: &str) -> LinkId {
        state
            .active_mut()
            .document_mut()
            .add_link(tessera_document::links::Link::new(path, (10.0, 10.0)))
    }

    #[test]
    fn rows_are_one_per_file_sorted_by_name_with_their_use_count() {
        let mut state = TesseraApp::headless();
        let zebra = linked(&mut state, "C:/art/zebra.png");
        let apple = linked(&mut state, "C:/art/Apple.png");
        for link in [zebra, zebra, apple] {
            showing(&mut state, link);
        }

        let rows = rows(&mut state);
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["Apple.png", "zebra.png"], "by name, case aside");
        assert_eq!(rows[1].uses, 2);
        assert_eq!(rows[0].status, Status::Missing, "nothing at C:/art");
        assert!(rows[0].page.is_some(), "the page it is on");
    }

    #[test]
    fn the_disk_is_not_asked_every_frame() {
        let mut state = TesseraApp::headless();
        linked(&mut state, "C:/art/photo.png");
        let _ = rows(&mut state);
        let asked = state.links.asked;
        let _ = rows(&mut state);
        assert_eq!(
            state.links.asked, asked,
            "the second read came from the cache"
        );
        state.links.recheck();
        let _ = rows(&mut state);
        assert_ne!(state.links.asked, asked, "and a recheck asks again");
    }

    #[test]
    fn choosing_a_row_selects_and_reveals_the_frame_showing_it() {
        let mut state = TesseraApp::headless();
        let link = linked(&mut state, "C:/art/photo.png");
        let id = showing(&mut state, link);
        state.active_mut().selection.clear();

        go_to(&mut state, link);
        assert_eq!(state.active().selection.single(), Some(id));
        assert_eq!(state.reveal, Some(id));
    }
}
