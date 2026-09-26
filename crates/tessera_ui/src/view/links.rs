//! The Links panel: every file the document points at, what the disk says
//! about each, and whether it will print well.
//!
//! What InDesign's Links panel is for, and what this one is for: a job goes
//! to a printer with its artwork, and the panel is where somebody sees, before
//! it goes, that one picture is a week old, another is not there at all, and a
//! third is stretched across a page at sixty pixels to the inch. One row per
//! file rather than per frame — the same photograph placed four times is one
//! thing to relink — with its picture, its size, the page it is on and how
//! many frames show it.
//!
//! **The problems are what it leads with.** The top of the panel counts what
//! is missing, changed and short of resolution, and each count narrows the
//! list to those files; beside them are the two things that fix most of it at
//! once — reading every changed file again, and finding every missing one in
//! a folder somebody moved them to.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};

use tessera_document::ids::{FrameId, LinkId};
use tessera_document::links::Status;

use super::style_ui;
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

/// A row's height: the picture, and two lines beside it.
const ROW: f32 = 46.0;

/// The picture at a row's start.
const THUMB: Vec2 = Vec2::new(46.0, 36.0);

/// How many pictures are read for the panel in one frame. The rest wait for
/// the next: a photograph is tens of milliseconds to decode, and a panel of
/// forty opening at once would hold the window still for seconds.
const THUMBS_PER_FRAME: u8 = 1;

/// The size pictures are read at for the panel, the details' larger one
/// included: the image cache keeps it, and a proxy on disk after that.
const THUMB_EDGE: u32 = 256;

/// Past this many files the list gets a search field.
const SEARCH_FROM: usize = 8;

#[derive(Debug, Clone, Default)]
pub struct LinksPanel {
    pub open: bool,
    /// The row chosen, whose details show below the list.
    pub selected: Option<LinkId>,
    /// Which files the list shows.
    pub filter: Filter,
    /// The list narrowed to names containing this.
    pub search: String,
    /// What the last search of a folder found, said until the next.
    pub report: Option<String>,
    /// What the disk said, and when it was asked.
    statuses: Vec<(LinkId, Status)>,
    asked: Option<Instant>,
    /// The document revision the statuses were read against.
    revision: u64,
    /// The chosen file's size and date, and when the disk was asked.
    file: Option<(PathBuf, Instant, Option<FileFacts>)>,
}

/// A file's size in bytes, and when it was last changed in seconds since
/// 1970 when the disk says.
type FileFacts = (u64, Option<u64>);

/// Which files the list shows: all of them, or the ones with one problem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Missing,
    Modified,
    LowResolution,
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
        self.file = None;
    }

    /// A file's size and date, asked of the disk as often as its status is.
    fn file(&mut self, path: &Path) -> Option<FileFacts> {
        if let Some((at, when, facts)) = &self.file
            && at == path
            && when.elapsed() <= STATUS_TTL
        {
            return *facts;
        }
        let facts = std::fs::metadata(path)
            .ok()
            .map(|m| (m.len(), tessera_document::links::modified_seconds(&m)));
        self.file = Some((path.to_path_buf(), Instant::now(), facts));
        facts
    }
}

/// One file, as the list shows it.
#[derive(Debug, Clone)]
struct Row {
    link: LinkId,
    name: String,
    path: PathBuf,
    status: Status,
    /// Every frame showing it, in reading order.
    frames: Vec<FrameId>,
    /// Where the first of them is.
    page: Spot,
    /// Its size as placed: pixels for a picture, points for a drawing.
    natural: (f64, f64),
    vector: bool,
    /// The lowest resolution it is printed at, across its uses.
    worst: Option<f64>,
}

impl Row {
    fn low(&self, minimum: f64) -> bool {
        self.status != Status::Missing && self.worst.is_some_and(|ppi| ppi < minimum)
    }

    fn shown(&self, filter: Filter, minimum: f64) -> bool {
        match filter {
            Filter::All => true,
            Filter::Missing => self.status == Status::Missing,
            Filter::Modified => self.status == Status::Modified,
            Filter::LowResolution => self.low(minimum),
        }
    }
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
            let page = frames.first().map_or(Spot::Pasteboard, |f| spot(doc, *f));
            let worst = frames
                .iter()
                .filter_map(|f| doc.effective_ppi(*f))
                .reduce(f64::min);
            Some(Row {
                link,
                name: l
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| l.path.to_string_lossy().into_owned()),
                path: l.path.clone(),
                status,
                frames,
                page,
                natural: l.natural,
                vector: l.is_vector(),
                worst,
            })
        })
        .collect();
    // By name, so a row can be found; the panel is a list to look a file up
    // in, not a history of what was placed when.
    rows.sort_by_key(|r| r.name.to_lowercase());
    rows
}

/// Where a frame is, as the panel says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Spot {
    /// On a document page, by its folio.
    Page(String),
    /// On a parent page, so on every page built on it; by the parent's name.
    Parent(String),
    Pasteboard,
}

impl Spot {
    /// For a row's corner: "p. 3", "A-Master".
    pub(crate) fn short(&self) -> String {
        match self {
            Spot::Page(folio) => format!("p.\u{2009}{folio}"),
            Spot::Parent(name) => name.clone(),
            Spot::Pasteboard => "pasteboard".to_string(),
        }
    }

    /// For a line of its own: "Page 3", "On A-Master".
    pub(crate) fn long(&self) -> String {
        match self {
            Spot::Page(folio) => format!("Page {folio}"),
            Spot::Parent(name) => format!("On {name}"),
            Spot::Pasteboard => "On the pasteboard".to_string(),
        }
    }
}

pub(crate) fn spot(doc: &tessera_document::document::Document, frame: FrameId) -> Spot {
    let Some(page) = doc.page_of_frame(frame) else {
        return Spot::Pasteboard;
    };
    if let Some(folio) = doc.page_label(page) {
        return Spot::Page(folio);
    }
    doc.master_ids()
        .find(|master| doc.pages_of_master(*master).contains(&page))
        .and_then(|master| doc.masters.get(master))
        .map_or(Spot::Pasteboard, |master| Spot::Parent(master.name.clone()))
}

/// The word and the colour a status is shown in.
fn describe(status: Status) -> (&'static str, Color32, Icon) {
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
    crate::view::styles::reveal_object(state, frame);
}

/// What kind of file it is, in words.
fn format(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "JPEG picture".to_string(),
        "png" => "PNG picture".to_string(),
        "tif" | "tiff" => "TIFF picture".to_string(),
        "webp" => "WebP picture".to_string(),
        "gif" => "GIF picture".to_string(),
        "bmp" => "BMP picture".to_string(),
        "svg" => "SVG drawing".to_string(),
        "" => "File".to_string(),
        other => format!("{} file", other.to_uppercase()),
    }
}

/// A file size as people say it: "812 KB", "1.4 MB".
fn bytes(n: u64) -> String {
    let n = n as f64;
    if n < 1024.0 {
        format!("{n:.0} bytes")
    } else if n < 1024.0 * 1024.0 {
        format!("{:.0} KB", n / 1024.0)
    } else if n < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", n / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", n / (1024.0 * 1024.0 * 1024.0))
    }
}

/// A moment, as a date: "12 Sep 2026". From days since 1970 by Howard
/// Hinnant's civil-from-days, which needs no calendar library for the one
/// date the panel shows.
fn date(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{day} {} {year}", MONTHS[(month - 1) as usize])
}

/// What a file manager is called here, for its button.
pub(crate) fn file_manager() -> &'static str {
    if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else if cfg!(target_os = "macos") {
        "Reveal in Finder"
    } else {
        "Show in folder"
    }
}

// --- pictures --------------------------------------------------------------------

/// The panel's pictures, by path, with the file date they were read at.
#[derive(Clone, Default)]
struct Thumbs(HashMap<PathBuf, (Option<u64>, egui::TextureHandle)>);

/// The picture of a file, for the panel: read into the image cache — which
/// the canvas shares, so a picture on the page is usually there already —
/// and kept as a texture until the file changes.
///
/// `None` for a file that is not there, one that cannot be read, or one whose
/// turn has not come this frame; then another frame is asked for.
fn thumbnail(
    ui: &Ui,
    state: &mut TesseraApp,
    path: &Path,
    budget: &mut u8,
) -> Option<egui::TextureHandle> {
    let tessera_io::seen::Seen::Present { modified } = tessera_io::seen::seen(path) else {
        return None;
    };
    let store = egui::Id::new("links-thumbnails");
    let held = ui.data(|d| {
        d.get_temp::<Thumbs>(store)
            .and_then(|t| t.0.get(path).cloned())
    });
    if let Some((at, texture)) = held
        && at == modified
    {
        return Some(texture);
    }
    if *budget == 0 {
        ui.ctx().request_repaint();
        return None;
    }
    *budget -= 1;
    let decoded = state.images.at_size(path, Some(THUMB_EDGE))?;
    let (w, h) = decoded.pixels;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        decoded.image.data.data(),
    );
    let texture = ui.ctx().load_texture(
        format!("link {}", path.display()),
        image,
        egui::TextureOptions::LINEAR,
    );
    ui.data_mut(|d| {
        d.get_temp_mut_or_default::<Thumbs>(store)
            .0
            .insert(path.to_path_buf(), (modified, texture.clone()));
    });
    Some(texture)
}

/// Draw a file's picture into `area`, whole, on the panel's ground; or, with
/// none to draw, the picture of what it is.
fn picture(ui: &Ui, area: Rect, texture: Option<&egui::TextureHandle>, row: &Row) {
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 5.0, Theme::panel_bg_solid());
    match texture {
        Some(texture) => {
            let size = texture.size_vec2();
            let scale = (area.width() / size.x).min(area.height() / size.y);
            let drawn = Rect::from_center_size(area.center(), size * scale);
            painter.image(
                texture.id(),
                drawn,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            let (icon, tint) = if row.status == Status::Missing {
                (Icon::Unlink2, Theme::error())
            } else if row.vector {
                (Icon::Pen, Theme::text_muted())
            } else {
                (Icon::PlaceImage, Theme::text_muted())
            };
            crate::icons::paint(&painter, area, icon, tint);
        }
    }
    painter.rect_stroke(
        area,
        5.0,
        Stroke::new(1.0, Theme::rule()),
        egui::StrokeKind::Inside,
    );
}

// --- the panel ---------------------------------------------------------------------

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
    let minimum = state.prefs.minimum_ppi;

    summary(ui, state, &rows, minimum);
    remedies(ui, state, &rows);
    if rows.len() > SEARCH_FROM {
        ui.add(
            egui::TextEdit::singleline(&mut state.links.search)
                .hint_text("Find a file")
                .desired_width(f32::INFINITY),
        );
    }
    let search = state.links.search.trim().to_lowercase();
    let filter = state.links.filter;
    let shown: Vec<&Row> = rows
        .iter()
        .filter(|r| r.shown(filter, minimum))
        .filter(|r| search.is_empty() || r.name.to_lowercase().contains(&search))
        .collect();

    // The list above the chosen file's details, and scrolling on its own when
    // it is longer than the room they leave: as tall as the card was last
    // frame, since a card is only measured by drawing it.
    let card = egui::Id::new("links-details-height");
    let details = if state.links.selected.is_some() {
        ui.data(|d| d.get_temp::<f32>(card)).unwrap_or(420.0) + Theme::space_2()
    } else {
        0.0
    };
    let room = (ui.clip_rect().bottom() - ui.cursor().top() - details).max(ROW * 3.0);
    let mut budget = THUMBS_PER_FRAME;
    let mut chosen = None;
    let mut asked = None;
    egui::ScrollArea::vertical()
        .id_salt("links-list")
        .max_height(room)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for row in &shown {
                let (clicked, act) = list_row(ui, state, row, minimum, &mut budget);
                if clicked {
                    chosen = Some(row.link);
                }
                if act.is_some() {
                    asked = act;
                }
            }
        });
    if shown.is_empty() {
        super::panel_ui::hint(ui, "No file here is like that.");
    }
    if let Some(link) = chosen {
        state.links.selected = Some(link);
        go_to(state, link);
    }
    if let Some((link, act)) = asked {
        run(ui.ctx(), state, link, act, &rows);
    }

    // The chosen file, in full.
    let Some(link) = state.links.selected else {
        return;
    };
    let Some(row) = rows.iter().find(|r| r.link == link).cloned() else {
        state.links.selected = None;
        return;
    };
    ui.add_space(Theme::space_2());
    // Scrolling too, for a window too short for both.
    let left = (ui.clip_rect().bottom() - ui.cursor().top()).max(ROW * 2.0);
    let drawn = egui::ScrollArea::vertical()
        .id_salt("links-details")
        .max_height(left)
        .auto_shrink([false, true])
        .show(ui, |ui| details_card(ui, state, &row, minimum, &mut budget))
        .content_size
        .y;
    if ui.data(|d| d.get_temp::<f32>(card)) != Some(drawn) {
        ui.data_mut(|d| d.insert_temp(card, drawn));
        ui.ctx().request_repaint();
    }
}

/// The counts, each a way to see only those files: how many there are, and
/// how many are missing, changed on disk and short of resolution.
fn summary(ui: &mut Ui, state: &mut TesseraApp, rows: &[Row], minimum: f64) {
    let missing = rows.iter().filter(|r| r.status == Status::Missing).count();
    let modified = rows.iter().filter(|r| r.status == Status::Modified).count();
    let low = rows.iter().filter(|r| r.low(minimum)).count();
    let files = if rows.len() == 1 {
        "1 file".to_string()
    } else {
        format!("{} files", rows.len())
    };
    let mut choice = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(4.0);
        for (filter, text, tint, count) in [
            (Filter::All, files, Theme::text_primary(), rows.len()),
            (
                Filter::Missing,
                format!("{missing} missing"),
                Theme::error(),
                missing,
            ),
            (
                Filter::Modified,
                format!("{modified} modified"),
                Theme::accent(),
                modified,
            ),
            (
                Filter::LowResolution,
                format!("{low} low resolution"),
                Theme::accent(),
                low,
            ),
        ] {
            if count == 0 && filter != Filter::All {
                continue;
            }
            let on = state.links.filter == filter;
            if chip(ui, &text, tint, on).clicked() {
                // A second click on a problem's count shows everything again.
                choice = Some(if on { Filter::All } else { filter });
            }
        }
    });
    if let Some(filter) = choice {
        state.links.filter = filter;
    }
    // A filter whose files have all been fixed shows the whole list again
    // rather than an empty one.
    let left = rows.iter().any(|r| r.shown(state.links.filter, minimum));
    if !left {
        state.links.filter = Filter::All;
    }
}

/// A count in a pill, lit when it is the filter in force.
pub(crate) fn chip(ui: &mut Ui, text: &str, tint: Color32, on: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(Theme::TYPE_SM),
        tint,
    );
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + Vec2::new(16.0, 8.0), Sense::click());
    let painter = ui.painter();
    painter.rect(
        rect,
        rect.height() / 2.0,
        if on {
            Theme::accent_soft()
        } else if response.hovered() {
            Theme::hover_bg()
        } else {
            Color32::TRANSPARENT
        },
        Stroke::new(
            1.0,
            if on {
                Theme::accent_edge()
            } else {
                Theme::rule()
            },
        ),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, tint);
    crate::icons::reads_as(response, text, egui::WidgetType::RadioButton, Some(on))
}

/// The two things that fix most problems at once, offered when there is
/// something for them to do.
fn remedies(ui: &mut Ui, state: &mut TesseraApp, rows: &[Row]) {
    let modified: Vec<LinkId> = rows
        .iter()
        .filter(|r| r.status == Status::Modified)
        .map(|r| r.link)
        .collect();
    let missing: Vec<(LinkId, String)> = rows
        .iter()
        .filter(|r| r.status == Status::Missing)
        .map(|r| (r.link, r.name.clone()))
        .collect();
    if modified.is_empty() && missing.is_empty() && state.links.report.is_none() {
        return;
    }
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        if !modified.is_empty()
            && super::panel_ui::action(ui, Icon::RotateCw, "Update all")
                .on_hover_text("Read every changed file again, in one step")
                .clicked()
        {
            apply(
                state,
                Command::UpdateLinks {
                    links: modified.clone(),
                },
            );
            state.links.recheck();
        }
        if !missing.is_empty()
            && super::panel_ui::action(ui, Icon::Link2, "Find missing…")
                .on_hover_text(
                    "Choose a folder: every missing file found in it, or in the folders \
                     inside it, is relinked there in one step",
                )
                .clicked()
        {
            find_missing(state);
        }
    });
    if let Some(report) = &state.links.report {
        super::panel_ui::hint(ui, report);
    }
    ui.add_space(2.0);
}

/// Ask for a folder, and relink every missing file found in it or the
/// folders inside it: the repair for a job whose artwork was moved.
pub(crate) fn find_missing(state: &mut TesseraApp) {
    let missing: Vec<(LinkId, String)> = rows(state)
        .into_iter()
        .filter(|r| r.status == Status::Missing)
        .map(|r| (r.link, r.name))
        .collect();
    if missing.is_empty() {
        return;
    }
    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
        relink_from(state, &folder, &missing);
    }
}

/// Open the Links panel with `link` chosen, its details showing.
pub(crate) fn show(state: &mut TesseraApp, link: LinkId) {
    state.links.open = true;
    state.links.selected = Some(link);
    state.links.filter = Filter::All;
    state.rail_open = true;
    state.prefs.docking.reveal("Links");
}

/// Relink each missing file to the file of the same name in `folder` or a
/// folder inside it, and say how many were found.
fn relink_from(state: &mut TesseraApp, folder: &Path, missing: &[(LinkId, String)]) {
    let names: Vec<&str> = missing.iter().map(|(_, n)| n.as_str()).collect();
    let found = find_by_name(folder, &names);
    let changes: Vec<(LinkId, PathBuf)> = missing
        .iter()
        .filter_map(|(link, name)| found.get(name.as_str()).map(|p| (*link, p.clone())))
        .collect();
    let count = changes.len();
    if count > 0 {
        apply(state, Command::RelinkMany { changes });
        state.links.recheck();
    }
    state.links.report = Some(match (count, missing.len()) {
        (0, _) => format!("None of the missing files is in {}.", folder.display()),
        (n, all) if n == all => format!("Found all {all} in {}.", folder.display()),
        (n, all) => format!("Found {n} of {all} in {}.", folder.display()),
    });
}

/// The first file with each of `names` in `folder` or a folder inside it,
/// nearest first: the folder somebody moved a job's artwork to usually holds
/// it all, sometimes a level or two down.
///
/// A file of that name, not of that name ignoring case, unless nothing else
/// is found — on a disk where case counts, `Photo.JPG` and `photo.jpg` are
/// two files. Bounded, so a folder chosen by mistake — a whole drive — is a
/// short wait and not a long one.
fn find_by_name<'a>(folder: &Path, names: &[&'a str]) -> HashMap<&'a str, PathBuf> {
    const DEEPEST: usize = 4;
    const MOST: usize = 50_000;
    let mut exact: HashMap<&str, PathBuf> = HashMap::new();
    let mut loose: HashMap<&str, PathBuf> = HashMap::new();
    let mut queue = std::collections::VecDeque::from([(folder.to_path_buf(), 0usize)]);
    let mut seen = 0;
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subfolders = Vec::new();
        for entry in entries.flatten() {
            seen += 1;
            if seen > MOST {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                if depth < DEEPEST {
                    subfolders.push(path);
                }
                continue;
            }
            let Some(file) = path.file_name().and_then(|f| f.to_str()) else {
                continue;
            };
            for name in names {
                if file == *name {
                    exact.entry(name).or_insert_with(|| path.clone());
                } else if file.eq_ignore_ascii_case(name) {
                    loose.entry(name).or_insert_with(|| path.clone());
                }
            }
        }
        if exact.len() == names.len() || seen > MOST {
            break;
        }
        subfolders.sort();
        queue.extend(subfolders.into_iter().map(|p| (p, depth + 1)));
    }
    for (name, path) in loose {
        exact.entry(name).or_insert(path);
    }
    exact
}

/// What a row's menu, or the details' buttons, asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Relink,
    Update,
    GoTo,
    Reveal,
    Open,
    CopyPath,
}

fn run(ctx: &egui::Context, state: &mut TesseraApp, link: LinkId, act: Act, rows: &[Row]) {
    let Some(row) = rows.iter().find(|r| r.link == link) else {
        return;
    };
    match act {
        Act::Relink => {
            if let Some(path) = crate::file_ops::pick_artwork() {
                apply(state, Command::Relink { link, path });
                state.links.recheck();
            }
        }
        Act::Update => {
            apply(state, Command::UpdateLink { link });
            state.links.recheck();
        }
        Act::GoTo => go_to(state, link),
        Act::Reveal => reveal_in_file_manager(&row.path),
        Act::Open => open_with_its_application(&row.path),
        Act::CopyPath => ctx.copy_text(row.path.to_string_lossy().into_owned()),
    }
}

/// One file: its picture, its name, what is true of it, where it is.
fn list_row(
    ui: &mut Ui,
    state: &mut TesseraApp,
    row: &Row,
    minimum: f64,
    budget: &mut u8,
) -> (bool, Option<(LinkId, Act)>) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
    let selected = state.links.selected == Some(row.link);
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 6.0, Theme::accent_soft());
    } else if response.hovered() {
        painter.rect_filled(rect, 6.0, Theme::hover_bg());
    }

    let thumb = Rect::from_min_size(
        egui::pos2(rect.left() + 4.0, rect.center().y - THUMB.y / 2.0),
        THUMB,
    );
    let texture = thumbnail(ui, state, &row.path, budget);
    picture(ui, thumb, texture.as_ref(), row);
    // What is wrong, on the picture's corner, where the eye goes first.
    let badge = match row.status {
        Status::Missing => Some(Theme::error()),
        Status::Modified => Some(Theme::accent()),
        Status::Fine if row.low(minimum) => Some(Theme::accent()),
        Status::Fine => None,
    };
    if let Some(tint) = badge {
        let at = thumb.right_top() + Vec2::new(-2.0, 2.0);
        painter.circle(at, 5.5, tint, Stroke::new(1.5, Theme::panel_bg_solid()));
    }

    let left = thumb.right() + 9.0;
    let small = egui::FontId::proportional(Theme::TYPE_SM);
    // At the right: the page it is on, and how many frames show it.
    let place = match row.frames.len() {
        0 => "unused".to_string(),
        1 => row.page.short(),
        n => format!("{} \u{00d7}{n}", row.page.short()),
    };
    let place = painter.layout_no_wrap(place, small.clone(), Theme::text_muted());
    let right = rect.right() - 8.0;
    let top = rect.top() + 8.0;
    painter.galley(
        egui::pos2(right - place.size().x, top + 1.0),
        place.clone(),
        Theme::text_muted(),
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        row.name.clone(),
        egui::TextStyle::Body.resolve(ui.style()),
        Theme::text_primary(),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (right - place.size().x - 8.0 - left).max(12.0),
    );
    let name = painter.layout_job(job);
    painter.galley(egui::pos2(left, top - 1.0), name, Theme::text_primary());

    // Under it, what matters about it: what is wrong, or what it is.
    let (line, tint) = match row.status {
        Status::Missing => ("Missing: not at its path".to_string(), Theme::error()),
        Status::Modified => (
            "Changed on disk since it was placed".to_string(),
            Theme::accent(),
        ),
        Status::Fine => (
            facts(row, minimum),
            if row.low(minimum) {
                Theme::accent()
            } else {
                Theme::text_muted()
            },
        ),
    };
    let mut job = egui::text::LayoutJob::simple_singleline(line, small, tint);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(12.0));
    let second = painter.layout_job(job);
    painter.galley(
        egui::pos2(left, rect.bottom() - 8.0 - second.size().y),
        second,
        tint,
    );

    let (word, ..) = describe(row.status);
    let response = crate::icons::reads_as(
        response,
        &row.name,
        egui::WidgetType::SelectableLabel,
        Some(selected),
    )
    .on_hover_text(format!("{} \u{2014} {word}", row.path.display()));
    let mut act = None;
    response.context_menu(|ui| {
        for (label, choice, enabled) in [
            ("Relink…", Act::Relink, true),
            ("Update", Act::Update, row.status == Status::Modified),
            ("Go to link", Act::GoTo, !row.frames.is_empty()),
            (file_manager(), Act::Reveal, row.status != Status::Missing),
            ("Open", Act::Open, row.status != Status::Missing),
            ("Copy path", Act::CopyPath, true),
        ] {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                act = Some((row.link, choice));
                ui.close();
            }
        }
    });
    (response.clicked(), act)
}

/// A file's size and resolution in a line: "2400 × 1600 px · 180 ppi".
fn facts(row: &Row, minimum: f64) -> String {
    let (w, h) = row.natural;
    if row.vector {
        return format!("Drawing \u{00b7} {w:.0} \u{00d7} {h:.0} pt");
    }
    let size = format!("{w:.0} \u{00d7} {h:.0} px");
    match row.worst {
        Some(ppi) if ppi < minimum => format!("{size} \u{00b7} {ppi:.0} ppi, too low"),
        Some(ppi) => format!("{size} \u{00b7} {ppi:.0} ppi"),
        None => size,
    }
}

/// The chosen file, in full: its picture, what it is, where it is, where it
/// is used, and what can be done with it.
fn details_card(ui: &mut Ui, state: &mut TesseraApp, row: &Row, minimum: f64, budget: &mut u8) {
    let texture = thumbnail(ui, state, &row.path, budget);
    let file = state.links.file(&row.path);
    let doc = state.active().document();
    let uses: Vec<(FrameId, Spot, Option<f64>)> = row
        .frames
        .iter()
        .map(|f| (*f, spot(doc, *f), doc.effective_ppi(*f)))
        .collect();
    let (word, tint, _) = describe(row.status);
    let mut act = None;
    let mut visit = None;

    style_ui::card(ui, None, |ui| {
        let width = ui.available_width();
        let (area, _) = ui.allocate_exact_size(Vec2::new(width, 116.0), Sense::hover());
        picture(ui, area, texture.as_ref(), row);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&row.name)
                        .font(style_ui::heading_font(15.0))
                        .color(Theme::text_primary()),
                )
                .truncate()
                .selectable(false),
            );
        });
        let (status, status_tint) = if row.low(minimum) && row.status == Status::Fine {
            ("Up to date, low resolution", Theme::accent())
        } else {
            (word, tint)
        };
        ui.colored_label(status_tint, status);
        ui.add_space(4.0);

        info_line(ui, "Kind", format(&row.path), Theme::text_primary());
        let (w, h) = row.natural;
        if row.vector {
            info_line(
                ui,
                "Size",
                format!("{w:.0} \u{00d7} {h:.0} pt"),
                Theme::text_primary(),
            );
        } else if w > 0.0 && h > 0.0 {
            info_line(
                ui,
                "Pixels",
                format!("{w:.0} \u{00d7} {h:.0}"),
                Theme::text_primary(),
            );
        }
        if let Some(ppi) = row.worst {
            let many = if uses.len() > 1 {
                ", the lowest of its uses"
            } else {
                ""
            };
            if ppi < minimum {
                info_line(
                    ui,
                    "Resolution",
                    format!("{ppi:.0} ppi{many}: below the {minimum:.0} asked for"),
                    Theme::accent(),
                );
            } else {
                info_line(
                    ui,
                    "Resolution",
                    format!("{ppi:.0} ppi{many}"),
                    Theme::text_primary(),
                );
            }
        }
        if let Some((size, modified)) = file {
            let changed = modified
                .map(|s| format!(", changed {}", date(s)))
                .unwrap_or_default();
            info_line(
                ui,
                "File",
                format!("{}{changed}", bytes(size)),
                Theme::text_primary(),
            );
        }
        // The folder, on one line, cut from the front: the end of a path is
        // what tells two copies of a file apart.
        let folder = row.path.parent().unwrap_or(&row.path);
        let fitted = fitted_path(
            ui,
            folder,
            ui.available_width() - LABEL - ui.spacing().item_spacing.x,
        );
        info_line(ui, "Folder", fitted, Theme::text_muted())
            .on_hover_text(row.path.display().to_string());

        if !uses.is_empty() {
            ui.add_space(6.0);
            style_ui::overline(
                ui,
                &if uses.len() == 1 {
                    "Placed once".to_string()
                } else {
                    format!("Placed {} times", uses.len())
                },
            );
            for (frame, place, ppi) in &uses {
                let text = match ppi {
                    Some(ppi) => format!("{} \u{00b7} {ppi:.0} ppi", place.long()),
                    None => place.long(),
                };
                let low = ppi.is_some_and(|p| p < minimum);
                if super::style_ui::page_link(ui, Icon::PictureFrame, &text, ui.available_width())
                    .on_hover_text(if low {
                        "Go to it. It prints below the resolution asked for."
                    } else {
                        "Go to it"
                    })
                    .clicked()
                {
                    visit = Some(*frame);
                }
            }
        }

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if super::panel_ui::action(ui, Icon::PlaceImage, "Relink…")
                .on_hover_text(
                    "Choose the file this should point at. Every frame showing it follows.",
                )
                .clicked()
            {
                act = Some(Act::Relink);
            }
            if row.status == Status::Modified
                && super::panel_ui::action(ui, Icon::RotateCw, "Update")
                    .on_hover_text("Read the changed file again")
                    .clicked()
            {
                act = Some(Act::Update);
            }
            ui.add_enabled_ui(row.status != Status::Missing, |ui| {
                if ui
                    .button(file_manager())
                    .on_hover_text("Open the folder it is in")
                    .clicked()
                {
                    act = Some(Act::Reveal);
                }
                if ui
                    .button("Open")
                    .on_hover_text("Open it in the application your system opens it with")
                    .clicked()
                {
                    act = Some(Act::Open);
                }
            });
        });
    });
    if let Some(frame) = visit {
        go_to_frame(state, frame);
    }
    if let Some(act) = act {
        run(ui.ctx(), state, row.link, act, std::slice::from_ref(row));
    }
}

/// The width of the names down the left of the details.
const LABEL: f32 = 78.0;

/// A fact about the chosen file: its name down the left, and what it is.
fn info_line(ui: &mut Ui, label: &str, value: String, tint: Color32) -> egui::Response {
    ui.horizontal(|ui| {
        let (name, _) = ui.allocate_exact_size(Vec2::new(LABEL, 18.0), Sense::hover());
        ui.painter().text(
            name.left_center(),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(Theme::TYPE_SM),
            Theme::text_muted(),
        );
        ui.add(egui::Label::new(egui::RichText::new(value).color(tint)).wrap())
    })
    .inner
}

/// `path` as it fits in `width`: whole if it can be, or its last folders after
/// an ellipsis.
fn fitted_path(ui: &Ui, path: &Path, width: f32) -> String {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let fits = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width
    };
    let whole = path.display().to_string();
    if fits(&whole) {
        return whole;
    }
    let parts: Vec<String> = path
        .components()
        .filter(|c| !matches!(c, std::path::Component::RootDir))
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let separator = std::path::MAIN_SEPARATOR.to_string();
    (1..parts.len())
        .map(|from| format!("\u{2026}{separator}{}", parts[from..].join(&separator)))
        .find(|tail| fits(tail))
        .unwrap_or_else(|| {
            format!(
                "\u{2026}{separator}{}",
                parts.last().cloned().unwrap_or(whole)
            )
        })
}

/// Open the file's folder with the file chosen, the way every file manager
/// can. Best effort: a manager that cannot is a link that is still fine.
pub(crate) fn reveal_in_file_manager(path: &Path) {
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

/// Open the file in whatever the system opens it with — InDesign's Edit
/// Original. Best effort, as revealing it is.
fn open_with_its_application(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
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

    /// A folder of its own under the temp directory, empty.
    fn a_folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tessera-links-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a folder");
        dir
    }

    /// A PNG `w` by `h` at `path`, and the folders it is in.
    fn a_png(path: &Path, w: u32, h: u32) -> PathBuf {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("its folder");
        }
        image::RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]))
            .save(path)
            .expect("write a png");
        path.to_path_buf()
    }

    /// A picture box `size` points square at `at`, showing `link`.
    fn showing_at(state: &mut TesseraApp, link: LinkId, at: tessera_geometry::DocRect) -> FrameId {
        apply(state, Command::AddGraphicFrame(at));
        let id = state.active().selection.single().expect("selected");
        // undo-bracketed: a test's own setup, not an edit a person makes.
        state.active_mut().document_mut().place(
            id,
            link,
            tessera_document::graphic::Fit::Proportionally,
        );
        id
    }

    /// A picture box on the first page, showing `link`.
    fn showing(state: &mut TesseraApp, link: LinkId) -> FrameId {
        let mut b = state.first_page_bounds();
        b.width = 50.0;
        b.height = 50.0;
        showing_at(state, link, b)
    }

    fn linked(state: &mut TesseraApp, path: &str) -> LinkId {
        // undo-bracketed: a test's own setup, not an edit a person makes.
        state
            .active_mut()
            .document_mut()
            .add_link(tessera_document::links::Link::new(path, (10.0, 10.0)))
    }

    /// `path` placed in a box `width` points square by the command a person
    /// uses, so the link is measured from the file.
    fn placed(state: &mut TesseraApp, path: &Path, width: f64) -> (FrameId, LinkId) {
        let mut b = state.first_page_bounds();
        b.width = width;
        b.height = width;
        apply(state, Command::AddGraphicFrame(b));
        let id = state.active().selection.single().expect("selected");
        apply(
            state,
            Command::PlaceArtwork {
                id,
                path: path.to_path_buf(),
                fit: tessera_document::graphic::Fit::Proportionally,
            },
        );
        (id, link_of(state, id).expect("placed"))
    }

    /// What a picture box shows.
    fn link_of(state: &TesseraApp, frame: FrameId) -> Option<LinkId> {
        match &state.active().document().frame(frame)?.kind {
            tessera_document::nodes::FrameKind::Graphic { placed: Some(p) } => Some(p.link),
            _ => None,
        }
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
        assert_eq!(rows[1].frames.len(), 2);
        assert_eq!(rows[0].status, Status::Missing, "nothing at C:/art");
        assert_eq!(rows[0].page, Spot::Page("1".into()), "the page it is on");
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

    #[test]
    fn artwork_on_a_parent_is_said_to_be_there_and_gone_to_there() {
        // A logo on the parent page is on every page and on none: the row
        // names the parent, and going to it opens the parent, where the frame
        // can be chosen, rather than choosing it where it cannot be seen.
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddMaster);
        let doc = state.active().document();
        let master = doc.master_order[0];
        let name = doc.masters[master].name.clone();
        let mut at = doc.pages[doc.pages_of_master(master)[0]].bounds;
        at.width = 40.0;
        at.height = 40.0;
        let link = linked(&mut state, "C:/art/logo.png");
        let id = showing_at(&mut state, link, at);
        state.active_mut().selection.clear();

        let rows = rows(&mut state);
        assert_eq!(rows[0].page, Spot::Parent(name.clone()));
        assert_eq!(rows[0].page.short(), name);
        assert_eq!(rows[0].page.long(), format!("On {name}"));

        go_to(&mut state, link);
        assert_eq!(state.editing_master, Some(master), "the parent is open");
        assert_eq!(state.active().selection.single(), Some(id));
    }

    #[test]
    fn a_place_reads_as_a_folio_a_parent_or_the_pasteboard() {
        let page = Spot::Page("iv".into());
        assert_eq!(page.short(), "p.\u{2009}iv");
        assert_eq!(page.long(), "Page iv");
        let parent = Spot::Parent("A-Master".into());
        assert_eq!(parent.short(), "A-Master");
        assert_eq!(parent.long(), "On A-Master");
        assert_eq!(Spot::Pasteboard.long(), "On the pasteboard");
    }

    #[test]
    fn a_picture_stretched_past_its_pixels_is_low_resolution() {
        let dir = a_folder("low");
        let mut state = TesseraApp::headless();
        // 30 pixels across 100 points: 21.6 to the inch.
        let (_, small) = placed(&mut state, &a_png(&dir.join("small.png"), 30, 20), 100.0);
        // 3000 across 100 points: 2160.
        let (_, large) = placed(
            &mut state,
            &a_png(&dir.join("large.png"), 3000, 2000),
            100.0,
        );
        let missing = linked(&mut state, "C:/art/gone.png");
        showing(&mut state, missing);

        let rows = rows(&mut state);
        let row = |link| rows.iter().find(|r| r.link == link).expect("a row");
        let minimum = 300.0;
        assert_eq!(row(small).status, Status::Fine);
        assert!((row(small).worst.expect("a picture") - 21.6).abs() < 0.1);
        assert!(row(small).low(minimum));
        assert!(!row(large).low(minimum));
        assert!(
            !row(missing).low(minimum),
            "a missing file is missing, not low: its pixels are not known"
        );
        assert_eq!(
            facts(row(small), minimum),
            "30 \u{00d7} 20 px \u{00b7} 22 ppi, too low"
        );
        assert_eq!(
            facts(row(large), minimum),
            "3000 \u{00d7} 2000 px \u{00b7} 2160 ppi"
        );

        let shown = |filter| -> Vec<LinkId> {
            rows.iter()
                .filter(|r| r.shown(filter, minimum))
                .map(|r| r.link)
                .collect()
        };
        assert_eq!(shown(Filter::LowResolution), vec![small]);
        assert_eq!(shown(Filter::Missing), vec![missing]);
        assert!(shown(Filter::Modified).is_empty());
        assert_eq!(shown(Filter::All).len(), 3);
    }

    #[test]
    fn a_file_placed_twice_is_as_good_as_its_worst_use() {
        // Fine in a thumbnail, stretched across a spread: the spread is what
        // the printer sees.
        let dir = a_folder("twice");
        let mut state = TesseraApp::headless();
        let path = a_png(&dir.join("photo.png"), 3000, 2000);
        let (_, link) = placed(&mut state, &path, 100.0);
        let (_, again) = placed(&mut state, &path, 1000.0);
        assert_eq!(again, link, "the same file is one link");

        let rows = rows(&mut state);
        assert_eq!(rows[0].frames.len(), 2);
        assert!((rows[0].worst.expect("a picture") - 216.0).abs() < 0.1);
        assert!(rows[0].low(300.0));
    }

    #[test]
    fn a_drawing_has_no_resolution_to_be_short_of() {
        let mut state = TesseraApp::headless();
        let link = linked(&mut state, "C:/art/mark.svg");
        showing(&mut state, link);
        let rows = rows(&mut state);
        assert!(rows[0].vector);
        assert_eq!(rows[0].worst, None);
        assert_eq!(facts(&rows[0], 300.0), "Drawing \u{00b7} 10 \u{00d7} 10 pt");
    }

    #[test]
    fn dates_sizes_and_kinds_read_as_people_say_them() {
        assert_eq!(date(0), "1 Jan 1970");
        assert_eq!(date(951_782_400), "29 Feb 2000", "a leap day");
        assert_eq!(date(1_000_000_000), "9 Sep 2001");
        assert_eq!(date(1_789_000_000), "10 Sep 2026");

        assert_eq!(bytes(512), "512 bytes");
        assert_eq!(bytes(2048), "2 KB");
        assert_eq!(bytes(1_572_864), "1.5 MB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 GB");

        assert_eq!(format(Path::new("a/photo.JPG")), "JPEG picture");
        assert_eq!(format(Path::new("mark.svg")), "SVG drawing");
        assert_eq!(format(Path::new("layout.psd")), "PSD file");
        assert_eq!(format(Path::new("README")), "File");
    }

    #[test]
    fn a_long_folder_keeps_its_end() {
        let ctx = a_panel();
        let path = Path::new("/home/someone/jobs/2026/autumn-catalogue/links");
        let mut short = String::new();
        let mut whole = String::new();
        let mut last = String::new();
        crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            short = fitted_path(ui, path, 180.0);
            whole = fitted_path(ui, path, 2000.0);
            last = fitted_path(ui, path, 1.0);
        });
        assert_eq!(whole, path.display().to_string(), "whole when it fits");
        let sep = std::path::MAIN_SEPARATOR;
        assert!(short.starts_with('\u{2026}'), "{short}");
        assert!(
            short.ends_with(&format!("autumn-catalogue{sep}links")),
            "{short}"
        );
        assert_eq!(last, format!("\u{2026}{sep}links"), "at least its own name");
    }

    #[test]
    fn the_chosen_file_s_size_is_not_asked_every_frame() {
        let dir = a_folder("facts");
        let path = a_png(&dir.join("photo.png"), 4, 4);
        let mut panel = LinksPanel::default();
        let size = panel.file(&path).expect("there").0;
        assert!(size > 0);
        std::fs::remove_file(&path).expect("remove");
        assert_eq!(panel.file(&path).map(|f| f.0), Some(size), "from the cache");
        panel.recheck();
        assert_eq!(panel.file(&path), None, "and asked again on a recheck");
    }

    #[test]
    fn a_folder_is_searched_nearest_first_and_by_exact_name_first() {
        let dir = a_folder("search");
        a_png(&dir.join("one.png"), 2, 2);
        a_png(&dir.join("sub/one.png"), 2, 2);
        a_png(&dir.join("sub/two.png"), 2, 2);
        a_png(&dir.join("Three.PNG"), 2, 2);
        a_png(&dir.join("a/b/three.png"), 2, 2);
        a_png(&dir.join("1/2/3/4/five.png"), 2, 2);
        a_png(&dir.join("1/2/3/4/5/six.png"), 2, 2);

        let found = find_by_name(
            &dir,
            &[
                "one.png",
                "two.png",
                "three.png",
                "five.png",
                "six.png",
                "four.png",
            ],
        );
        assert_eq!(found["one.png"], dir.join("one.png"), "the nearest");
        assert_eq!(found["two.png"], dir.join("sub/two.png"), "a level down");
        assert_eq!(
            found["three.png"],
            dir.join("a/b/three.png"),
            "the name exactly, deeper, over the name in another case"
        );
        assert_eq!(found["five.png"], dir.join("1/2/3/4/five.png"));
        assert!(!found.contains_key("six.png"), "deeper than it looks");
        assert!(!found.contains_key("four.png"), "not there at all");

        // With nothing better, a name in another case will do.
        let loose = find_by_name(&dir, &["three.PNG"]);
        assert_eq!(loose["three.PNG"], dir.join("Three.PNG"));
    }

    #[test]
    fn finding_moved_artwork_relinks_all_of_it_in_one_step() {
        let before = a_folder("moved-from");
        let mut state = TesseraApp::headless();
        let (first, a) = placed(&mut state, &a_png(&before.join("a.png"), 20, 10), 50.0);
        let (_, b) = placed(&mut state, &a_png(&before.join("b.png"), 20, 10), 50.0);
        let c = linked(&mut state, "C:/art/c.png");
        state.links.selected = Some(a);
        let after = a_folder("moved-to");
        std::fs::rename(before.join("a.png"), after.join("a.png")).expect("move");
        a_png(&after.join("deeper/b.png"), 40, 20);
        // One copy of `a` already placed from where it went: relinking the
        // rest to it joins that link, and the chosen row goes with them.
        let (_, there) = placed(&mut state, &after.join("a.png"), 50.0);
        let depth = state.active().history.undo_depth();

        let missing = [
            (a, "a.png".to_string()),
            (b, "b.png".to_string()),
            (c, "c.png".to_string()),
        ];
        relink_from(&mut state, &after, &missing);

        let now = link_of(&state, first).expect("still placed");
        assert_eq!(now, there, "joined the link already there");
        let doc = state.active().document();
        assert_eq!(doc.links[now].path, after.join("a.png"));
        let b_now = doc
            .links
            .values()
            .find(|l| l.path == after.join("deeper/b.png"))
            .expect("b found a level down");
        assert_eq!(b_now.natural, (40.0, 20.0), "measured from the file found");
        assert_eq!(state.active().history.undo_depth(), depth + 1, "one step");
        assert_eq!(state.links.selected, Some(now), "the row stays chosen");
        assert_eq!(
            state.links.report.as_deref(),
            Some(format!("Found 2 of 3 in {}.", after.display()).as_str())
        );

        relink_from(&mut state, &after, &[(c, "c.png".to_string())]);
        assert_eq!(
            state.active().history.undo_depth(),
            depth + 1,
            "nothing found is nothing done"
        );
        assert!(
            state
                .links
                .report
                .as_deref()
                .is_some_and(|r| r.starts_with("None of the missing files"))
        );
    }

    #[test]
    fn updating_every_changed_file_is_one_step() {
        let dir = a_folder("update");
        let mut state = TesseraApp::headless();
        let one = a_png(&dir.join("one.png"), 10, 10);
        let two = a_png(&dir.join("two.png"), 10, 10);
        let (_, a) = placed(&mut state, &one, 50.0);
        let (_, b) = placed(&mut state, &two, 50.0);
        a_png(&one, 30, 30);
        a_png(&two, 60, 40);
        let depth = state.active().history.undo_depth();

        apply(&mut state, Command::UpdateLinks { links: vec![a, b] });

        let doc = state.active().document();
        assert_eq!(doc.links[a].natural, (30.0, 30.0));
        assert_eq!(doc.links[b].natural, (60.0, 40.0));
        assert_eq!(state.active().history.undo_depth(), depth + 1);
        apply(&mut state, Command::Undo);
        assert_eq!(state.active().document().links[b].natural, (10.0, 10.0));
    }

    // --- the panel, used -------------------------------------------------------

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

    fn labels(ctx: &egui::Context, state: &mut TesseraApp) -> Vec<String> {
        panel(ctx, state, Vec::new());
        panel(ctx, state, Vec::new())
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        panel(ctx, state, Vec::new());
        let nodes = panel(ctx, state, Vec::new());
        let pos = nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
            .center();
        for pressed in [true, false] {
            panel(
                ctx,
                state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }

    /// One file that prints well and one that is not there.
    fn one_fine_one_missing() -> (TesseraApp, LinkId, LinkId) {
        let dir = a_folder("panel");
        let mut state = TesseraApp::headless();
        let (_, fine) = placed(&mut state, &a_png(&dir.join("fine.png"), 3000, 3000), 100.0);
        let missing = linked(&mut state, "C:/art/gone.png");
        showing(&mut state, missing);
        state.active_mut().selection.clear();
        (state, fine, missing)
    }

    #[test]
    fn a_problem_s_count_shows_only_those_files_and_a_second_click_all() {
        let (mut state, ..) = one_fine_one_missing();
        let ctx = a_panel();
        let all = labels(&ctx, &mut state);
        for label in ["2 files", "1 missing", "fine.png", "gone.png"] {
            assert!(all.iter().any(|l| l == label), "{label:?} in {all:?}");
        }
        assert!(
            !all.iter().any(|l| l.contains("modified")),
            "a count of nothing is not shown: {all:?}"
        );

        click(&ctx, &mut state, "1 missing");
        assert_eq!(state.links.filter, Filter::Missing);
        let narrowed = labels(&ctx, &mut state);
        assert!(narrowed.iter().any(|l| l == "gone.png"));
        assert!(!narrowed.iter().any(|l| l == "fine.png"), "{narrowed:?}");

        click(&ctx, &mut state, "1 missing");
        assert_eq!(state.links.filter, Filter::All);
    }

    #[test]
    fn a_filter_whose_files_are_all_fixed_shows_the_whole_list() {
        let (mut state, _, missing) = one_fine_one_missing();
        state.links.filter = Filter::Missing;
        let ctx = a_panel();
        labels(&ctx, &mut state);
        assert_eq!(state.links.filter, Filter::Missing);
        // undo-bracketed: a test's own setup, not an edit a person makes.
        state.active_mut().document_mut().links.remove(missing);
        labels(&ctx, &mut state);
        assert_eq!(state.links.filter, Filter::All, "not an empty list");
    }

    #[test]
    fn choosing_a_row_shows_what_is_true_of_the_file_and_what_can_be_done() {
        let (mut state, fine, _) = one_fine_one_missing();
        let ctx = a_panel();
        assert!(!labels(&ctx, &mut state).iter().any(|l| l == "Relink…"));

        click(&ctx, &mut state, "fine.png");
        assert_eq!(state.links.selected, Some(fine));
        assert!(state.active().selection.single().is_some(), "and gone to");
        let shown = labels(&ctx, &mut state);
        for label in [
            "Relink…",
            file_manager(),
            "Open",
            "Page 1 \u{00b7} 2160 ppi",
        ] {
            assert!(shown.iter().any(|l| l == label), "{label:?} in {shown:?}");
        }
        assert!(
            !shown.iter().any(|l| l == "Update"),
            "nothing to update in a file that has not changed"
        );
    }
}
