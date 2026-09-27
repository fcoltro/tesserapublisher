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
//!
//! The list is kept as well as used: narrowed to a kind or to what nothing
//! uses, put in order by dragging or by name, filled from the colours the
//! document already uses unnamed, and passed to and from InDesign,
//! Illustrator and Photoshop as a swatch exchange file.

use std::path::Path;

use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};

use tessera_color::Color;
use tessera_document::nodes::Swatch;
use tessera_document::paint::Paint;

use super::swatch_editor::{self, BLACK, BLACK_INK, NONE, PAPER, PAPER_INK};
use crate::app::{SwatchShow, SwatchTarget, TesseraApp};
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// A row's height: a chip with air round it, and a name that reads.
const ROW: f32 = 26.0;
/// The colour chip on each row.
const CHIP: Vec2 = Vec2::new(26.0, 16.0);
/// A tile's side, when the colours are shown as tiles.
const TILE: f32 = 30.0;
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

    /// What it puts on an object at `tint`: nothing, for [None]; a swatch
    /// by reference, so editing the swatch recolours everything it is on;
    /// the black plate at that share, which is what a grey is in print.
    fn colour_at(&self, tint: f32) -> Option<Color> {
        match self {
            Self::None => None,
            Self::Paper => Some(PAPER_INK),
            Self::Black if tint < 1.0 => Some(BLACK_INK.tinted(tint)),
            Self::Black => Some(BLACK_INK),
            Self::Named(name) => Some(Color::Swatch {
                name: name.clone(),
                tint,
            }),
        }
    }

    /// Whether a tint of it means anything: paper at half strength is still
    /// paper, and nothing is nothing.
    pub(crate) fn tints(&self) -> bool {
        matches!(self, Self::Black | Self::Named(_))
    }

    /// The entry a colour on the page is, if it is one of the list's, and
    /// the tint it is at.
    fn of(colour: &Color) -> Option<(Self, f32)> {
        match colour {
            Color::Swatch { name, tint } => Some((Self::Named(name.clone()), *tint)),
            c if *c == PAPER_INK => Some((Self::Paper, 1.0)),
            // The black plate alone, at any share: [Black] at a tint.
            Color::Cmyk { c, m, y, k, a }
                if *c == 0.0 && *m == 0.0 && *y == 0.0 && *k > 0.0 && *a >= 1.0 =>
            {
                Some((Self::Black, *k))
            }
            c if c.to_rgb_f32()[3] <= 0.0 => Some((Self::None, 1.0)),
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
    tint: f32,
) -> Option<Vec<Command>> {
    let colour = entry.colour_at(tint);
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

/// Colour the selection with `entry` at `tint`, if it has anything to
/// colour: one step to undo however many objects it colours.
pub(crate) fn apply_entry(state: &mut TesseraApp, entry: &Entry, tint: f32) {
    let target = state.swatches_window.target;
    if let Some(commands) = apply_commands(state, entry, target, tint) {
        apply(state, Command::Together(commands));
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
#[cfg(test)]
fn on_selection(state: &TesseraApp) -> Option<Entry> {
    on_selection_at(state).map(|(entry, _)| entry)
}

/// The same, with the tint it is at.
fn on_selection_at(state: &TesseraApp) -> Option<(Entry, f32)> {
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
                None => Some((Entry::None, 1.0)),
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
    let places = |name: &str| {
        counts
            .iter()
            .find(|(n, _)| n == name)
            .map_or(0, |(_, n)| *n)
    };
    let current = on_selection_at(state);
    let unused: Vec<String> = swatches
        .iter()
        .filter(|s| places(&s.name) == 0)
        .map(|s| s.name.clone())
        .collect();

    let mut asked = header(ui, state, &swatches, unused.len());
    ui.add_space(Theme::space_2());
    ui.horizontal(|ui| {
        ui.colored_label(Theme::text_muted(), "Apply to");
        target_choice(ui, state);
    });
    ui.add_space(Theme::space_2());

    // A kind with nothing left in it — its last unused swatch deleted —
    // shows everything again rather than an empty list.
    let show = state.swatches_window.show;
    if !swatches.iter().any(|s| show.takes(s, places(&s.name))) {
        state.swatches_window.show = SwatchShow::All;
    }
    if !swatches.is_empty() {
        show_choice(ui, state, &swatches, &places);
        ui.add_space(Theme::space_1());
    }
    if swatches.len() > FILTER_FROM {
        ui.add(
            egui::TextEdit::singleline(&mut state.swatches_window.filter)
                .hint_text("Filter swatches")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(Theme::space_1());
    }
    let filter = state.swatches_window.filter.trim().to_lowercase();
    let show = state.swatches_window.show;
    let shown: Vec<&Swatch> = swatches
        .iter()
        .filter(|s| show.takes(s, places(&s.name)))
        .filter(|s| filter.is_empty() || s.name.to_lowercase().contains(&filter))
        .collect();

    let list = List {
        all: &swatches,
        shown: &shown,
        built_ins: show == SwatchShow::All,
        places: &places,
        current: current.as_ref().map(|(entry, _)| entry),
        chosen: state.swatches_window.chosen.clone(),
    };
    let picked = if state.prefs.swatch_tiles {
        tiles(ui, state, &list)
    } else {
        rows(ui, state, &list)
    };

    if swatches.is_empty() {
        super::panel_ui::empty(
            ui,
            "No named colours yet",
            "Create a swatch from the selected object's fill, load a swatch file, or name the \
             colours the document already uses from the menu.",
        );
    } else if shown.is_empty() {
        super::panel_ui::hint(ui, "No swatch has that in its name.");
    }
    if show == SwatchShow::Unused && !unused.is_empty() {
        ui.add_space(Theme::space_1());
        if super::panel_ui::action(ui, Icon::Trash, &count("Delete unused", unused.len()))
            .on_hover_text("Delete every swatch nothing uses, in one step to undo")
            .clicked()
        {
            asked = Some(Whole::DeleteUnused);
        }
    }
    if let Some(note) = &state.swatches_window.note {
        ui.add_space(Theme::space_1());
        super::panel_ui::hint(ui, note);
    }

    ui.add_space(Theme::space_2());
    actions(ui, state, current.clone());

    if let Some(entry) = picked.choose {
        // The colour the selection wears, chosen, offers its own tint back;
        // any other starts at full strength.
        state.swatches_window.tint = current
            .as_ref()
            .filter(|(on, _)| *on == entry)
            .map(|(_, tint)| *tint)
            .filter(|tint| *tint < 1.0);
        state.swatches_window.chosen = Some(entry.name().to_owned());
        state.swatches_window.note = None;
    }
    if let Some(name) = picked.open {
        state.swatches_window.chosen = Some(name);
        state.swatches_window.editing = true;
    }
    if let Some((name, action)) = picked.menu {
        state.swatches_window.chosen = Some(name.clone());
        run(state, &name, action);
    }
    if let Some((name, before)) = picked.moved {
        apply(state, Command::MoveSwatch { name, before });
    }
    if let Some(whole) = asked {
        whole_list(state, whole, &unused);
    }
}

/// "Delete unused (3)".
fn count(label: &str, n: usize) -> String {
    format!("{label} ({n})")
}

/// "1 swatch", "3 swatches".
fn swatches_said(n: usize) -> String {
    match n {
        1 => "1 swatch".to_string(),
        n => format!("{n} swatches"),
    }
}

impl SwatchShow {
    const ALL: [Self; 5] = [
        Self::All,
        Self::Process,
        Self::Spot,
        Self::Tints,
        Self::Unused,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Process => "Process",
            Self::Spot => "Spot",
            Self::Tints => "Tints",
            Self::Unused => "Unused",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::All => "Every swatch, and the built-in colours",
            Self::Process => "Colours printed as a mix of the process inks",
            Self::Spot => "Inks of their own, each printed on a plate of its own",
            Self::Tints => "Swatches that are a share of another",
            Self::Unused => "Swatches nothing in the document uses",
        }
    }

    /// Whether the list shows `swatch`, which `places` things use.
    fn takes(self, swatch: &Swatch, places: usize) -> bool {
        let tint = matches!(swatch.colour, Color::Swatch { .. });
        let spot = swatch_editor::is_spot(swatch);
        match self {
            Self::All => true,
            Self::Process => !tint && !spot,
            Self::Spot => !tint && spot,
            Self::Tints => tint,
            Self::Unused => places == 0,
        }
    }
}

/// The kinds, each with how many swatches it holds: a kind with none is
/// not offered, and a click on the kind already shown shows them all.
fn show_choice(
    ui: &mut Ui,
    state: &mut TesseraApp,
    swatches: &[Swatch],
    places: &dyn Fn(&str) -> usize,
) {
    let now = state.swatches_window.show;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for show in SwatchShow::ALL {
            let n = swatches
                .iter()
                .filter(|s| show.takes(s, places(&s.name)))
                .count();
            if n == 0 && show != SwatchShow::All {
                continue;
            }
            let tint = if show == SwatchShow::Unused {
                Theme::text_muted()
            } else {
                Theme::text_primary()
            };
            if super::links::chip(ui, &format!("{} {n}", show.label()), tint, now == show)
                .on_hover_text(show.hint())
                .clicked()
            {
                state.swatches_window.show = if now == show { SwatchShow::All } else { show };
            }
        }
    });
}

/// What the header's menu does to the whole list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Whole {
    Load,
    Save,
    NameUnnamed,
    DeleteUnused,
    Sort,
}

/// New swatch at the left; at the right, rows or tiles, and the menu of
/// what is done to the list as a whole.
fn header(
    ui: &mut Ui,
    state: &mut TesseraApp,
    swatches: &[Swatch],
    unused: usize,
) -> Option<Whole> {
    let mut asked = None;
    ui.horizontal(|ui| {
        if super::panel_ui::action(ui, Icon::Plus, "New swatch")
            .on_hover_text("Name the selected object's fill, or a plain black")
            .clicked()
        {
            let swatch = fresh(state);
            let window = &mut state.swatches_window;
            window.chosen = Some(swatch.name.clone());
            window.editing = true;
            window.note = None;
            apply(state, Command::SetSwatch(swatch));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let menu = ui.menu_button(egui::RichText::new("\u{2022}\u{2022}\u{2022}"), |ui| {
                let mut item =
                    |ui: &mut Ui, enabled: bool, label: &str, hint: &str, whole: Whole| {
                        if ui
                            .add_enabled(enabled, egui::Button::new(label))
                            .on_hover_text(hint)
                            .clicked()
                        {
                            asked = Some(whole);
                            ui.close();
                        }
                    };
                item(
                    ui,
                    true,
                    "Load swatches\u{2026}",
                    "From an Adobe Swatch Exchange file (.ase), or from another Tessera document",
                    Whole::Load,
                );
                item(
                    ui,
                    !swatches.is_empty(),
                    "Save swatches\u{2026}",
                    "As an Adobe Swatch Exchange file, for InDesign, Illustrator and Photoshop",
                    Whole::Save,
                );
                ui.separator();
                let unnamed = unnamed_count(state);
                item(
                    ui,
                    unnamed > 0,
                    &count("Add unnamed colours", unnamed),
                    "Make a swatch of every colour used without one, and point its uses at it",
                    Whole::NameUnnamed,
                );
                item(
                    ui,
                    unused > 0,
                    &count("Delete unused swatches", unused),
                    "Delete every swatch nothing uses, in one step to undo",
                    Whole::DeleteUnused,
                );
                let sorted = swatches
                    .windows(2)
                    .all(|w| tessera_document::name_order(&w[0].name, &w[1].name).is_le());
                item(
                    ui,
                    !sorted,
                    "Sort by name",
                    "Put the swatches in order of their names",
                    Whole::Sort,
                );
            });
            crate::icons::named(menu.response, "More swatch options");
            let tiles = state.prefs.swatch_tiles;
            if super::panels::icon_button(ui, Icon::ViewGrid, "Show as tiles", tiles) {
                state.prefs.swatch_tiles = true;
            }
            if super::panels::icon_button(ui, Icon::ViewList, "Show as a list", !tiles) {
                state.prefs.swatch_tiles = false;
            }
        });
    });
    asked
}

/// How many colours the document uses without a swatch: counted while the
/// menu offering to name them is open, once for each change to the
/// document, since every colour in it is read to say.
fn unnamed_count(state: &mut TesseraApp) -> usize {
    let document = state.active;
    let revision = state.active().document().revision();
    match state.swatches_window.unnamed {
        Some((counted, at, n)) if counted == document && at == revision => n,
        _ => {
            let n = state
                .active()
                .document()
                .unnamed_colours(&[PAPER_INK, BLACK_INK])
                .len();
            state.swatches_window.unnamed = Some((document, revision, n));
            n
        }
    }
}

/// Do something to the whole list, and say what came of it.
fn whole_list(state: &mut TesseraApp, whole: Whole, unused: &[String]) {
    let note = match whole {
        Whole::Load => ask_load(state),
        Whole::Save => ask_save(state),
        Whole::NameUnnamed => {
            let before = state.active().document().swatches.len();
            apply(state, Command::NameUnnamedColours);
            let made = state.active().document().swatches.len() - before;
            Some(format!(
                "Named {}: each is a swatch now, and what used it uses the swatch.",
                match made {
                    1 => "1 colour".to_string(),
                    n => format!("{n} colours"),
                }
            ))
        }
        Whole::DeleteUnused => {
            apply(
                state,
                Command::Together(
                    unused
                        .iter()
                        .map(|name| Command::RemoveSwatch { name: name.clone() })
                        .collect(),
                ),
            );
            let window = &mut state.swatches_window;
            if window
                .chosen
                .as_ref()
                .is_some_and(|chosen| unused.contains(chosen))
            {
                window.chosen = None;
            }
            Some(format!(
                "Deleted {} nothing used.",
                swatches_said(unused.len())
            ))
        }
        Whole::Sort => {
            apply(state, Command::SortSwatches);
            None
        }
    };
    if note.is_some() {
        state.swatches_window.note = note;
    }
}

/// Ask for a file of swatches and bring them in; what came of it, or
/// nothing when the dialog was put away.
fn ask_load(state: &mut TesseraApp) -> Option<String> {
    let path = rfd::FileDialog::new()
        .set_title("Load swatches")
        .add_filter("Swatches", &["ase", crate::file_ops::EXTENSION])
        .add_filter("Adobe Swatch Exchange", &["ase"])
        .add_filter("Tessera document", &[crate::file_ops::EXTENSION])
        .pick_file()?;
    Some(load_from(state, &path).unwrap_or_else(|problem| problem))
}

/// Ask where to write the swatches, and write them there.
fn ask_save(state: &mut TesseraApp) -> Option<String> {
    let stem = state
        .active()
        .current_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map_or_else(
            || "Untitled".to_string(),
            |s| s.to_string_lossy().into_owned(),
        );
    let mut path = rfd::FileDialog::new()
        .set_title("Save swatches")
        .add_filter("Adobe Swatch Exchange", &["ase"])
        .set_file_name(format!("{stem} swatches.ase"))
        .save_file()?;
    if path.extension().is_none() {
        path.set_extension("ase");
    }
    Some(save_to(state, &path).unwrap_or_else(|problem| problem))
}

/// The file's name, as a sentence says it.
fn file_said(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Bring in the swatches of an `.ase` file or of another document, and say
/// what came in. Built-in colours another program lists — its `[Black]`,
/// its `[Registration]` — are not brought: the names are this panel's own.
pub(crate) fn load_from(state: &mut TesseraApp, path: &Path) -> Result<String, String> {
    let file = file_said(path);
    let exchange = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ase"));
    let incoming: Vec<Swatch> = if exchange {
        let bytes = std::fs::read(path).map_err(|e| format!("Could not read {file}: {e}"))?;
        tessera_document::swatch_exchange::read(&bytes).map_err(|e| format!("{file}: {e}"))?
    } else {
        tessera_document::format::load(path)
            .map_err(|e| format!("Could not open {file}: {e}"))?
            .swatches
    };
    let incoming: Vec<Swatch> = incoming
        .into_iter()
        .filter(|s| !(s.name.starts_with('[') && s.name.ends_with(']')))
        .collect();
    if incoming.is_empty() {
        return Ok(format!("{file} has no swatches to bring in."));
    }
    // Merged into a copy first, for the sentence: the command merges the
    // document itself, and says nothing.
    let merge = state
        .active()
        .document()
        .clone()
        .add_swatches(incoming.clone());
    let new = merge.added.len() + merge.renamed.len();
    if new == 0 {
        return Ok(format!(
            "Nothing new in {file}: {} here already.",
            match merge.already {
                1 => "its 1 swatch is".to_string(),
                n => format!("all {n} of its swatches are"),
            }
        ));
    }
    apply(state, Command::AddSwatches(incoming));
    let mut said = format!("Brought in {} from {file}", swatches_said(new));
    if !merge.renamed.is_empty() {
        said.push_str(&format!(
            "; {} under a new name, as this document gives the name another colour",
            merge.renamed.len()
        ));
    }
    if merge.already > 0 {
        said.push_str(&format!("; {} here already", merge.already));
    }
    said.push('.');
    Ok(said)
}

/// Write the swatches to an `.ase` file, and say what went.
pub(crate) fn save_to(state: &TesseraApp, path: &Path) -> Result<String, String> {
    let file = file_said(path);
    let doc = state.active().document();
    let out = doc.swatches_for_exchange();
    std::fs::write(path, tessera_document::swatch_exchange::write(&out))
        .map_err(|e| format!("Could not write {file}: {e}"))?;
    let mut said = format!("Saved {} to {file}", swatches_said(out.len()));
    let left = doc.swatches.len() - out.len();
    if left > 0 {
        said.push_str(&format!(
            "; {} naming a swatch that is not there left out",
            left
        ));
    }
    said.push('.');
    Ok(said)
}

/// What the list is asked to show.
struct List<'a> {
    /// Every swatch, in the document's order.
    all: &'a [Swatch],
    /// The ones shown, in that order.
    shown: &'a [&'a Swatch],
    /// Whether [None], [Paper] and [Black] head it.
    built_ins: bool,
    places: &'a dyn Fn(&str) -> usize,
    /// What the selection is coloured with.
    current: Option<&'a Entry>,
    chosen: Option<String>,
}

impl List<'_> {
    /// The swatch to put a dragged one before, dropped at `slot` among the
    /// shown: the one there, or — past the last shown — the one after it
    /// in the whole list, so a filtered list moves a swatch where it looks
    /// to go.
    fn before(&self, slot: usize) -> Option<String> {
        match self.shown.get(slot) {
            Some(swatch) => Some(swatch.name.clone()),
            None => {
                let last = self.shown.last()?;
                self.all
                    .iter()
                    .skip_while(|s| s.name != last.name)
                    .nth(1)
                    .map(|s| s.name.clone())
            }
        }
    }

    fn colour_of(&self, doc: &tessera_document::document::Document, swatch: &Swatch) -> [f32; 4] {
        doc.resolve_colour(&Color::Swatch {
            name: swatch.name.clone(),
            tint: 1.0,
        })
        .to_rgb_f32()
    }
}

/// What a click, a double-click, a menu or a drop in the list asked for.
#[derive(Default)]
struct Picked {
    choose: Option<Entry>,
    open: Option<String>,
    menu: Option<(String, MenuAction)>,
    moved: Option<(String, Option<String>)>,
}

impl Picked {
    /// A swatch's own row or tile answered: chosen, opened, or its menu.
    fn swatch(&mut self, swatch: &Swatch, response: &egui::Response, all: &[Swatch]) {
        if response.clicked() {
            self.choose = Some(Entry::Named(swatch.name.clone()));
        }
        if response.double_clicked() {
            self.open = Some(swatch.name.clone());
        }
        let at = all.iter().position(|s| s.name == swatch.name);
        response.context_menu(|ui| {
            for action in MenuAction::ALL {
                let offered = match action {
                    MenuAction::NewTint => !matches!(swatch.colour, Color::Swatch { .. }),
                    MenuAction::MoveUp => at.is_some_and(|at| at > 0),
                    MenuAction::MoveDown => at.is_some_and(|at| at + 1 < all.len()),
                    _ => true,
                };
                if !offered {
                    continue;
                }
                if matches!(action, MenuAction::MoveUp | MenuAction::Delete) {
                    ui.separator();
                }
                if ui.button(action.label()).clicked() {
                    self.menu = Some((swatch.name.clone(), action));
                    ui.close();
                }
            }
        });
    }

    /// A drag let go of at `slot`.
    fn dropped(&mut self, state: &mut TesseraApp, list: &List<'_>, slot: usize) {
        if let Some(name) = state.swatches_window.moving.take() {
            let before = list.before(slot);
            if before.as_deref() != Some(name.as_str()) {
                self.moved = Some((name, before));
            }
        }
    }
}

/// A row of the list, following a drag.
fn follow_drag(
    state: &mut TesseraApp,
    swatch: &Swatch,
    response: &egui::Response,
    pointer: &mut Option<egui::Pos2>,
    released: &mut bool,
) {
    if response.drag_started() {
        state.swatches_window.moving = Some(swatch.name.clone());
    }
    if response.dragged() || response.drag_stopped() {
        *pointer = response.interact_pointer_pos().or(*pointer);
    }
    if response.drag_stopped() {
        *released = true;
    }
}

/// The colours as rows: chip, name, the space it is written in, and how
/// many places use it. A row drags to a new place in the list.
fn rows(ui: &mut Ui, state: &mut TesseraApp, list: &List<'_>) -> Picked {
    let mut picked = Picked::default();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        if list.built_ins {
            for entry in [Entry::None, Entry::Paper, Entry::Black] {
                let shown = entry.colour_at(1.0).map(|c| c.to_rgb_f32());
                let response = row(
                    ui,
                    &Row {
                        name: entry.name(),
                        shown,
                        spot: false,
                        badge: None,
                        count: None,
                        chosen: list.chosen.as_deref() == Some(entry.name()),
                        current: list.current == Some(&entry),
                    },
                    Sense::click(),
                )
                .on_hover_text("Built in: always here, and not edited");
                if response.clicked() {
                    picked.choose = Some(entry);
                }
            }
            if !list.shown.is_empty() {
                ui.add_space(3.0);
                let rule = ui.cursor().top();
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    rule,
                    Stroke::new(1.0, Theme::rule()),
                );
                ui.add_space(4.0);
            }
        }

        let mut rects: Vec<Rect> = Vec::new();
        let mut pointer = None;
        let mut released = false;
        for swatch in list.shown {
            let places = (list.places)(&swatch.name);
            let shown = list.colour_of(state.active().document(), swatch);
            let is_chosen = list.chosen.as_deref() == Some(swatch.name.as_str());
            let moving = state.swatches_window.moving.as_deref() == Some(swatch.name.as_str());
            let response = ui
                .push_id(&swatch.name, |ui| {
                    row(
                        ui,
                        &Row {
                            name: &swatch.name,
                            shown: Some(shown),
                            spot: swatch_editor::is_spot(swatch),
                            badge: Some(badge(swatch)),
                            count: Some(places),
                            chosen: is_chosen || moving,
                            current: list.current == Some(&Entry::Named(swatch.name.clone())),
                        },
                        Sense::click_and_drag(),
                    )
                })
                .inner
                .on_hover_text(format!(
                    "{} Double-click to edit; drag to move.",
                    use_sentence(places)
                ));
            rects.push(response.rect);
            follow_drag(state, swatch, &response, &mut pointer, &mut released);
            picked.swatch(swatch, &response, list.all);
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

        // While a swatch is dragged: a line where it would land.
        if state.swatches_window.moving.is_some()
            && let Some(at) = pointer
        {
            let slot = rects
                .iter()
                .position(|r| at.y < r.center().y)
                .unwrap_or(rects.len());
            if released {
                picked.dropped(state, list, slot);
            } else if let Some(y) = rects
                .get(slot)
                .map(|r| r.top() - 1.0)
                .or_else(|| rects.last().map(|r| r.bottom() + 1.0))
            {
                let x = rects[0].x_range();
                ui.painter().line_segment(
                    [egui::pos2(x.min + 4.0, y), egui::pos2(x.max - 4.0, y)],
                    Stroke::new(2.0, Theme::accent()),
                );
            }
        } else if released {
            state.swatches_window.moving = None;
        }
    });
    picked
}

/// The colours as tiles, wrapped: a palette to look along, each named on
/// hover. A tile drags to a new place too.
fn tiles(ui: &mut Ui, state: &mut TesseraApp, list: &List<'_>) -> Picked {
    let mut picked = Picked::default();
    let mut rects: Vec<Rect> = Vec::new();
    let mut pointer = None;
    let mut released = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(5.0);
        if list.built_ins {
            for entry in [Entry::None, Entry::Paper, Entry::Black] {
                let response = tile(
                    ui,
                    &Tile {
                        name: entry.name(),
                        shown: entry.colour_at(1.0).map(|c| c.to_rgb_f32()),
                        spot: false,
                        chosen: list.chosen.as_deref() == Some(entry.name()),
                        current: list.current == Some(&entry),
                    },
                    Sense::click(),
                )
                .on_hover_text(format!("{} \u{00b7} built in", entry.name()));
                if response.clicked() {
                    picked.choose = Some(entry);
                }
            }
        }
        for swatch in list.shown {
            let places = (list.places)(&swatch.name);
            let is_chosen = list.chosen.as_deref() == Some(swatch.name.as_str());
            let moving = state.swatches_window.moving.as_deref() == Some(swatch.name.as_str());
            let shown = list.colour_of(state.active().document(), swatch);
            let response = ui
                .push_id(&swatch.name, |ui| {
                    tile(
                        ui,
                        &Tile {
                            name: &swatch.name,
                            shown: Some(shown),
                            spot: swatch_editor::is_spot(swatch),
                            chosen: is_chosen || moving,
                            current: list.current == Some(&Entry::Named(swatch.name.clone())),
                        },
                        Sense::click_and_drag(),
                    )
                })
                .inner
                .on_hover_text(format!(
                    "{} \u{00b7} {}\n{} Double-click to edit; drag to move.",
                    swatch.name,
                    swatch_editor::numbers(&swatch.colour),
                    use_sentence(places)
                ));
            rects.push(response.rect);
            follow_drag(state, swatch, &response, &mut pointer, &mut released);
            picked.swatch(swatch, &response, list.all);
        }
    });

    // While a tile is dragged: an upright line in the gap it would land in.
    if state.swatches_window.moving.is_some()
        && let Some(at) = pointer
    {
        let slot = rects
            .iter()
            .position(|r| at.y < r.top() || (at.y <= r.bottom() && at.x < r.center().x))
            .unwrap_or(rects.len());
        if released {
            picked.dropped(state, list, slot);
        } else if let Some((x, r)) = rects
            .get(slot)
            .map(|r| (r.left() - 3.0, *r))
            .or_else(|| rects.last().map(|r| (r.right() + 3.0, *r)))
        {
            ui.painter().line_segment(
                [egui::pos2(x, r.top()), egui::pos2(x, r.bottom())],
                Stroke::new(2.0, Theme::accent()),
            );
        }
    } else if released {
        state.swatches_window.moving = None;
    }
    if !list.shown.is_empty() || list.built_ins {
        ui.add_space(Theme::space_1());
    }
    // The chosen swatch under the tiles, by name and numbers: a tile says
    // nothing itself.
    if let Some(chosen) = &list.chosen {
        let said = match list.all.iter().find(|s| s.name == *chosen) {
            Some(swatch) => format!(
                "{chosen} \u{00b7} {}",
                swatch_editor::numbers(&swatch.colour)
            ),
            None => format!("{chosen} \u{00b7} built in"),
        };
        ui.add(
            egui::Label::new(
                egui::RichText::new(said)
                    .size(Theme::TYPE_SM)
                    .color(Theme::text_muted()),
            )
            .truncate()
            .selectable(false),
        );
    }
    picked
}

/// What a tile shows.
struct Tile<'a> {
    name: &'a str,
    shown: Option<[f32; 4]>,
    spot: bool,
    chosen: bool,
    current: bool,
}

/// One tile: the colour; the chosen one ringed in the accent, the one the
/// selection wears marked with a dot at its corner.
fn tile(ui: &mut Ui, tile: &Tile<'_>, sense: Sense) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(TILE), sense);
    let painter = ui.painter_at(rect.expand(4.0));
    match tile.shown {
        Some(rgba) => swatch_editor::chip(&painter, rect, rgba, 5.0, tile.spot),
        None => no_colour(&painter, rect, 5.0),
    }
    let ring = if tile.chosen {
        Some(Stroke::new(2.0, Theme::accent()))
    } else if response.hovered() {
        Some(Stroke::new(1.0, Theme::text_muted()))
    } else if response.has_focus() {
        Some(Stroke::new(1.0, Theme::focus()))
    } else {
        None
    };
    if let Some(ring) = ring {
        painter.rect_stroke(rect.expand(2.5), 7.0, ring, egui::StrokeKind::Middle);
    }
    if tile.current {
        let centre = rect.left_top() + Vec2::splat(6.0);
        painter.circle(
            centre,
            3.5,
            Theme::accent(),
            Stroke::new(1.0, Color32::WHITE),
        );
    }
    let name = if tile.current {
        format!("{}, on the selection", tile.name)
    } else {
        tile.name.to_owned()
    };
    crate::icons::reads_as(
        response,
        name,
        egui::WidgetType::SelectableLabel,
        Some(tile.chosen),
    )
}

/// [None]'s picture: paper struck through in red.
fn no_colour(painter: &egui::Painter, rect: Rect, radius: f32) {
    painter.rect_filled(rect, radius, Color32::WHITE);
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(1.5, Color32::from_rgb(0xE0, 0x30, 0x30)),
    );
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Theme::border()),
        egui::StrokeKind::Inside,
    );
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
fn row(ui: &mut Ui, row: &Row<'_>, sense: Sense) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), sense);
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
        None => no_colour(&painter, chip, 4.0),
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
    MoveUp,
    MoveDown,
    Delete,
}

impl MenuAction {
    const ALL: [Self; 6] = [
        Self::Edit,
        Self::NewTint,
        Self::Duplicate,
        Self::MoveUp,
        Self::MoveDown,
        Self::Delete,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Edit => "Edit swatch…",
            Self::NewTint => "New tint swatch",
            Self::Duplicate => "Duplicate swatch",
            Self::MoveUp => "Move up",
            Self::MoveDown => "Move down",
            Self::Delete => "Delete swatch…",
        }
    }
}

fn run(state: &mut TesseraApp, name: &str, action: MenuAction) {
    state.swatches_window.note = None;
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
        MenuAction::MoveUp | MenuAction::MoveDown => {
            let names: Vec<String> = state
                .active()
                .document()
                .swatches
                .iter()
                .map(|s| s.name.clone())
                .collect();
            let Some(at) = names.iter().position(|n| n == name) else {
                return;
            };
            // Up: before the one above. Down: before the one two below, or
            // to the end.
            let before = if action == MenuAction::MoveUp {
                match at.checked_sub(1) {
                    Some(above) => Some(names[above].clone()),
                    None => return,
                }
            } else {
                if at + 1 >= names.len() {
                    return;
                }
                names.get(at + 2).cloned()
            };
            apply(
                state,
                Command::MoveSwatch {
                    name: name.to_owned(),
                    before,
                },
            );
        }
        MenuAction::Delete => swatch_editor::delete(state, name),
    }
}

/// What can be done with the chosen entry: apply it, at a tint, and for a
/// swatch of the document's own, open it, make a tint swatch of it,
/// duplicate or delete it.
fn actions(ui: &mut Ui, state: &mut TesseraApp, current: Option<(Entry, f32)>) {
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
    let tint = entry
        .as_ref()
        .filter(|entry| entry.tints())
        .and(state.swatches_window.tint)
        .unwrap_or(1.0);
    let ready = entry
        .as_ref()
        .is_some_and(|entry| apply_commands(state, entry, target, tint).is_some());
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
        if entry.as_ref().is_some_and(Entry::tints) {
            ui.colored_label(Theme::text_muted(), "Tint");
            let mut percent = (tint * 100.0).round();
            let field = ui
                .add(
                    egui::DragValue::new(&mut percent)
                        .range(0.0..=100.0)
                        .speed(1.0)
                        .max_decimals(0)
                        .suffix("%"),
                )
                .on_hover_text(
                    "The tint Apply puts it on at: 100% is the colour itself, and less is \
                     lighter, toward the paper",
                );
            let field = crate::icons::speak_as(field, "Tint");
            if field.changed() {
                state.swatches_window.tint = (percent < 100.0).then_some(percent / 100.0);
            }
        }
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
    // What the selection is coloured with, when the chosen colour is it: at
    // which tint, since the list's dot does not say.
    if let (Some(entry), Some((on, at))) = (&entry, &current)
        && entry == on
        && *at < 1.0
    {
        super::panel_ui::hint(
            ui,
            &format!("The selection has it at {}%.", (at * 100.0).round() as i32),
        );
    }
    if apply_now && let Some(entry) = &entry {
        apply_entry(state, entry, tint);
        state.swatches_window.note = None;
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
        apply_entry(&mut state, &Entry::Named("Brand".into()), 1.0);
        let now = state.active().document().frames[id]
            .stroke
            .clone()
            .expect("a stroke");
        assert_eq!(now.color, brand());
        assert_eq!((now.width, now.dashes), (4.0, vec![3.0, 2.0]));

        apply_entry(&mut state, &Entry::None, 1.0);
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
            apply_commands(&state, &Entry::None, SwatchTarget::Text, 1.0).is_none(),
            "text has to be some colour"
        );
        apply_entry(&mut state, &Entry::Named("Brand".into()), 1.0);
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
            assert!(apply_commands(&state, &Entry::Named("Brand".into()), target, 1.0).is_none());
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

    fn cmyk(c: f32, m: f32, y: f32, k: f32) -> Color {
        Color::Cmyk { c, m, y, k, a: 1.0 }
    }

    fn names(state: &TesseraApp) -> Vec<String> {
        state
            .active()
            .document()
            .swatches
            .iter()
            .map(|s| s.name.clone())
            .collect()
    }

    /// A process colour on a rectangle, a spot ink, a tint of the process
    /// colour, and a grey nothing uses.
    fn a_palette() -> (TesseraApp, tessera_document::ids::FrameId) {
        let mut state = TesseraApp::headless();
        for swatch in [
            Swatch::new("Brand", cmyk(0.0, 0.91, 0.76, 0.0)),
            Swatch {
                name: "Ink".into(),
                colour: cmyk(1.0, 0.75, 0.0, 0.02),
                spot: true,
            },
            Swatch::new(
                "Brand 40%",
                Color::Swatch {
                    name: "Brand".into(),
                    tint: 0.4,
                },
            ),
            Swatch::new("Grey", cmyk(0.0, 0.0, 0.0, 0.45)),
        ] {
            apply(&mut state, Command::SetSwatch(swatch));
        }
        let id = a_rectangle(&mut state);
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: Paint::Solid(brand()),
            },
        );
        (state, id)
    }

    fn labels(ctx: &egui::Context, state: &mut TesseraApp) -> Vec<String> {
        panel(ctx, state, Vec::new());
        panel(ctx, state, Vec::new())
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    fn rect_of(ctx: &egui::Context, state: &mut TesseraApp, label: &str) -> egui::Rect {
        let nodes = panel(ctx, state, Vec::new());
        nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
    }

    /// Half a second of frames with nothing done: longer than a double
    /// click takes.
    fn pause(ctx: &egui::Context, state: &mut TesseraApp) {
        for _ in 0..40 {
            panel(ctx, state, Vec::new());
        }
    }

    /// Press on `from`, move in steps to `to`, and let go there.
    fn drag(ctx: &egui::Context, state: &mut TesseraApp, from: &str, to: egui::Pos2) {
        let at = rect_of(ctx, state, from).center();
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        panel(
            ctx,
            state,
            vec![egui::Event::PointerMoved(at), button(at, true)],
        );
        for step in 1..=6 {
            let p = at + (to - at) * (step as f32 / 6.0);
            panel(ctx, state, vec![egui::Event::PointerMoved(p)]);
        }
        panel(ctx, state, vec![button(to, false)]);
        panel(ctx, state, Vec::new());
    }

    #[test]
    fn apply_puts_the_chosen_swatch_on_at_the_tint_asked_in_one_step() {
        let (mut state, first) = a_palette();
        let second = a_rectangle(&mut state);
        state.active_mut().selection.set(first);
        state.active_mut().selection.add(second);
        let ctx = a_panel();
        press_on(&ctx, &mut state, "Grey", 1);
        state.swatches_window.tint = Some(0.25);
        press_on(&ctx, &mut state, "Apply", 1);
        let at_quarter = Paint::Solid(Color::Swatch {
            name: "Grey".into(),
            tint: 0.25,
        });
        for id in [first, second] {
            assert_eq!(state.active().document().frames[id].fill, at_quarter);
        }
        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().frames[first].fill,
            Paint::Solid(brand()),
            "both objects came back with one undo"
        );
        assert_ne!(state.active().document().frames[second].fill, at_quarter);
    }

    #[test]
    fn black_at_a_tint_is_the_black_plate_at_that_share() {
        assert_eq!(Entry::Black.colour_at(0.5), Some(cmyk(0.0, 0.0, 0.0, 0.5)));
        assert_eq!(
            Entry::of(&cmyk(0.0, 0.0, 0.0, 0.5)),
            Some((Entry::Black, 0.5))
        );
        assert_eq!(Entry::of(&BLACK_INK), Some((Entry::Black, 1.0)));
        assert_eq!(
            Entry::of(&cmyk(0.1, 0.0, 0.0, 0.5)),
            None,
            "a rich black is not [Black]"
        );
        assert_eq!(Entry::Paper.colour_at(0.5), Some(PAPER_INK));
        assert!(!Entry::Paper.tints() && !Entry::None.tints());
        assert!(Entry::Black.tints() && Entry::Named("Brand".into()).tints());
    }

    #[test]
    fn choosing_the_colour_the_selection_wears_offers_its_tint_back() {
        let (mut state, id) = a_palette();
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: Paint::Solid(Color::Swatch {
                    name: "Brand".into(),
                    tint: 0.3,
                }),
            },
        );
        let ctx = a_panel();
        press_on(&ctx, &mut state, "Brand", 1);
        assert_eq!(state.swatches_window.tint, Some(0.3));
        press_on(&ctx, &mut state, "Grey", 1);
        assert_eq!(state.swatches_window.tint, None, "another starts whole");
        assert!(labels(&ctx, &mut state).iter().any(|l| l == "Tint"));
        press_on(&ctx, &mut state, "[Paper]", 1);
        assert!(
            !labels(&ctx, &mut state).iter().any(|l| l == "Tint"),
            "no tint of paper is offered"
        );
    }

    #[test]
    fn the_kinds_narrow_the_list_and_say_how_many_each_holds() {
        let (mut state, _) = a_palette();
        let ctx = a_panel();
        let shown = labels(&ctx, &mut state);
        for chip in ["All 4", "Process 2", "Spot 1", "Tints 1", "Unused 3"] {
            assert!(shown.iter().any(|l| l == chip), "no {chip} in {shown:?}");
        }
        press_on(&ctx, &mut state, "Spot 1", 1);
        assert_eq!(state.swatches_window.show, SwatchShow::Spot);
        let shown = labels(&ctx, &mut state);
        assert!(shown.iter().any(|l| l == "Ink"));
        for gone in ["Brand", "Grey", "[Black]"] {
            assert!(!shown.iter().any(|l| l == gone), "{gone} in {shown:?}");
        }
        press_on(&ctx, &mut state, "Spot 1", 1);
        assert_eq!(state.swatches_window.show, SwatchShow::All, "again: all");
    }

    #[test]
    fn a_kind_with_nothing_left_in_it_shows_everything() {
        let (mut state, _) = a_palette();
        state.swatches_window.show = SwatchShow::Unused;
        for name in ["Ink", "Brand 40%", "Grey"] {
            apply(&mut state, Command::RemoveSwatch { name: name.into() });
        }
        let ctx = a_panel();
        labels(&ctx, &mut state);
        assert_eq!(state.swatches_window.show, SwatchShow::All);
    }

    #[test]
    fn delete_unused_takes_every_unused_swatch_in_one_step() {
        let (mut state, _) = a_palette();
        let ctx = a_panel();
        press_on(&ctx, &mut state, "Unused 3", 1);
        state.swatches_window.chosen = Some("Grey".into());
        press_on(&ctx, &mut state, "Delete unused (3)", 1);
        assert_eq!(names(&state), ["Brand"]);
        assert_eq!(state.swatches_window.chosen, None, "the chosen one went");
        assert_eq!(
            state.swatches_window.note.as_deref(),
            Some("Deleted 3 swatches nothing used.")
        );
        apply(&mut state, Command::Undo);
        assert_eq!(names(&state), ["Brand", "Ink", "Brand 40%", "Grey"]);
    }

    #[test]
    fn the_unnamed_colours_are_named_from_the_menu() {
        let mut state = TesseraApp::headless();
        let id = a_rectangle(&mut state);
        let teal = cmyk(0.8, 0.0, 0.4, 0.0);
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: Paint::Solid(teal.clone()),
            },
        );
        // A new rectangle's stroke is a colour of its own too; this is about
        // the fill.
        apply(&mut state, Command::SetStroke { id, stroke: None });
        assert_eq!(unnamed_count(&mut state), 1);
        whole_list(&mut state, Whole::NameUnnamed, &[]);
        assert_eq!(
            unnamed_count(&mut state),
            0,
            "counted again after the change"
        );
        assert_eq!(names(&state), ["C=80 M=0 Y=40 K=0"]);
        assert_eq!(
            state.active().document().frames[id].fill,
            Paint::Solid(Color::Swatch {
                name: "C=80 M=0 Y=40 K=0".into(),
                tint: 1.0
            })
        );
        assert_eq!(
            state.swatches_window.note.as_deref(),
            Some("Named 1 colour: each is a swatch now, and what used it uses the swatch.")
        );
        apply(&mut state, Command::Undo);
        assert!(names(&state).is_empty());
        assert_eq!(
            state.active().document().frames[id].fill,
            Paint::Solid(teal),
            "one undo takes the swatch and the repointing back"
        );
    }

    #[test]
    fn sort_by_name_puts_the_list_in_order() {
        let (mut state, _) = a_palette();
        whole_list(&mut state, Whole::Sort, &[]);
        assert_eq!(names(&state), ["Brand", "Brand 40%", "Grey", "Ink"]);
    }

    #[test]
    fn a_swatch_moves_up_and_down_from_its_menu() {
        let (mut state, _) = a_palette();
        run(&mut state, "Grey", MenuAction::MoveUp);
        assert_eq!(names(&state), ["Brand", "Ink", "Grey", "Brand 40%"]);
        run(&mut state, "Brand", MenuAction::MoveDown);
        assert_eq!(names(&state), ["Ink", "Brand", "Grey", "Brand 40%"]);
        run(&mut state, "Brand 40%", MenuAction::MoveDown);
        run(&mut state, "Ink", MenuAction::MoveUp);
        assert_eq!(
            names(&state),
            ["Ink", "Brand", "Grey", "Brand 40%"],
            "the ends stay put"
        );
    }

    #[test]
    fn a_row_dragged_lands_before_the_row_it_is_let_go_on() {
        let (mut state, _) = a_palette();
        let ctx = a_panel();
        labels(&ctx, &mut state);
        let ink = rect_of(&ctx, &mut state, "Ink");
        drag(
            &ctx,
            &mut state,
            "Grey",
            ink.center() - egui::vec2(0.0, 6.0),
        );
        assert_eq!(names(&state), ["Brand", "Grey", "Ink", "Brand 40%"]);
        let last = rect_of(&ctx, &mut state, "Brand 40%");
        drag(
            &ctx,
            &mut state,
            "Brand",
            last.center() + egui::vec2(0.0, 10.0),
        );
        assert_eq!(names(&state), ["Grey", "Ink", "Brand 40%", "Brand"]);
        assert_eq!(state.swatches_window.moving, None);
        apply(&mut state, Command::Undo);
        assert_eq!(names(&state), ["Brand", "Grey", "Ink", "Brand 40%"]);
    }

    #[test]
    fn a_tile_dragged_lands_before_the_tile_it_is_let_go_on() {
        let (mut state, _) = a_palette();
        state.prefs.swatch_tiles = true;
        let ctx = a_panel();
        labels(&ctx, &mut state);
        let brand = rect_of(&ctx, &mut state, "Brand, on the selection");
        drag(
            &ctx,
            &mut state,
            "Grey",
            brand.left_center() + egui::vec2(3.0, 0.0),
        );
        assert_eq!(names(&state), ["Grey", "Brand", "Ink", "Brand 40%"]);
        // Let go of on a tile's right half: after it.
        let grey = rect_of(&ctx, &mut state, "Grey");
        drag(
            &ctx,
            &mut state,
            "Brand 40%",
            grey.right_center() - egui::vec2(3.0, 0.0),
        );
        assert_eq!(names(&state), ["Grey", "Brand 40%", "Brand", "Ink"]);
    }

    #[test]
    fn the_list_can_be_shown_as_tiles_and_a_tile_chooses() {
        let (mut state, _) = a_palette();
        let ctx = a_panel();
        press_on(&ctx, &mut state, "Show as tiles", 1);
        assert!(state.prefs.swatch_tiles);
        let shown = labels(&ctx, &mut state);
        assert!(
            shown.iter().any(|l| l == "Brand, on the selection"),
            "the tile the selection wears says so: {shown:?}"
        );
        // A person's next click comes later than the frame after: without
        // the pause the click on the tile would be the second of a double
        // click that began on the button.
        pause(&ctx, &mut state);
        press_on(&ctx, &mut state, "Ink", 1);
        assert_eq!(state.swatches_window.chosen.as_deref(), Some("Ink"));
        assert!(!state.swatches_window.editing);
        press_on(&ctx, &mut state, "Ink", 2);
        assert!(state.swatches_window.editing);
        press_on(&ctx, &mut state, "Show as a list", 1);
        assert!(!state.prefs.swatch_tiles);
    }

    #[test]
    fn a_dropped_swatch_lands_before_the_next_one_in_the_whole_list() {
        let all = vec![
            Swatch::new("A", Color::BLACK),
            Swatch::new("B", Color::BLACK),
            Swatch::new("C", Color::BLACK),
            Swatch::new("D", Color::BLACK),
        ];
        let shown = [&all[0], &all[2]];
        let places = |_: &str| 0;
        let list = List {
            all: &all,
            shown: &shown,
            built_ins: false,
            places: &places,
            current: None,
            chosen: None,
        };
        assert_eq!(list.before(0).as_deref(), Some("A"));
        assert_eq!(list.before(1).as_deref(), Some("C"));
        assert_eq!(
            list.before(2).as_deref(),
            Some("D"),
            "past the last shown: before the one after it"
        );
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tessera-swatches-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    #[test]
    fn swatches_saved_to_a_file_are_loaded_into_another_document() {
        let (state, _) = a_palette();
        let path = temp("palette.ase");
        assert_eq!(
            save_to(&state, &path).as_deref(),
            Ok("Saved 4 swatches to palette.ase.")
        );

        let mut other = TesseraApp::headless();
        apply(
            &mut other,
            Command::SetSwatch(Swatch::new("Brand", cmyk(0.5, 0.0, 0.0, 0.0))),
        );
        assert_eq!(
            load_from(&mut other, &path).as_deref(),
            Ok(
                "Brought in 4 swatches from palette.ase; 1 under a new name, as this \
                document gives the name another colour."
            )
        );
        assert_eq!(
            names(&other),
            ["Brand", "Brand 2", "Ink", "Brand 40%", "Grey"]
        );
        let ink = other.active().document().swatch("Ink").cloned().unwrap();
        assert!(ink.spot, "a spot arrives a spot");
        let Color::Cmyk { c, m, y, k, .. } = other
            .active()
            .document()
            .swatch("Brand 40%")
            .unwrap()
            .colour
        else {
            panic!("a tint arrives as a colour");
        };
        assert!(
            [c, m - 0.364, y - 0.304, k].iter().all(|v| v.abs() < 1e-5),
            "a tint arrives as the colour it made: {c} {m} {y} {k}"
        );
        assert_eq!(
            load_from(&mut other, &path).as_deref(),
            Ok("Nothing new in palette.ase: all 4 of its swatches are here already.")
        );
        apply(&mut other, Command::Undo);
        assert_eq!(names(&other), ["Brand"], "the load was one step");
    }

    #[test]
    fn swatches_are_loaded_from_another_tessera_document_too() {
        let (state, _) = a_palette();
        let path = temp("brand.tsrdf");
        tessera_document::format::save(state.active().document(), &path).expect("saved");
        let mut other = TesseraApp::headless();
        load_from(&mut other, &path).expect("loaded");
        assert_eq!(names(&other), ["Brand", "Ink", "Brand 40%", "Grey"]);
        assert_eq!(
            other
                .active()
                .document()
                .swatch("Brand 40%")
                .unwrap()
                .colour,
            Color::Swatch {
                name: "Brand".into(),
                tint: 0.4
            },
            "from a document a tint stays a tint of its base"
        );
    }

    #[test]
    fn another_programs_built_in_colours_are_not_brought_in() {
        let path = temp("indesign.ase");
        let swatches = [
            Swatch::new("[Registration]", cmyk(1.0, 1.0, 1.0, 1.0)),
            Swatch::new("[Black]", cmyk(0.0, 0.0, 0.0, 1.0)),
            Swatch::new("Leaf", cmyk(0.6, 0.0, 1.0, 0.0)),
        ];
        std::fs::write(&path, tessera_document::swatch_exchange::write(&swatches)).unwrap();
        let mut state = TesseraApp::headless();
        load_from(&mut state, &path).expect("loaded");
        assert_eq!(names(&state), ["Leaf"]);
    }

    #[test]
    fn a_file_that_is_not_swatches_says_so_and_changes_nothing() {
        let path = temp("notes.ase");
        std::fs::write(&path, b"not a swatch file").unwrap();
        let mut state = TesseraApp::headless();
        assert_eq!(
            load_from(&mut state, &path),
            Err("notes.ase: This is not an Adobe Swatch Exchange file.".to_string())
        );
        assert!(names(&state).is_empty());
        let missing = temp("missing.ase");
        assert!(
            load_from(&mut state, &missing)
                .unwrap_err()
                .starts_with("Could not read missing.ase")
        );
    }
}
