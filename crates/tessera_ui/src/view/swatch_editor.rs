//! The Swatch window: one named colour — its numbers, how it will print, and
//! everything in the document that uses it.
//!
//! **A window, as a style is edited in one.** The panel is a list, read every
//! few minutes; changing what a colour *is* is done now and then, wants room
//! for four sliders and two previews, and wants the list still in view beside
//! it while it happens.
//!
//! What it shows that the panel's inline picker did not:
//!
//! - **The colour in its own numbers.** A CMYK swatch is edited as inks and
//!   stays CMYK. The old picker was RGB only, and touching a CMYK swatch
//!   through it turned it into RGB, separations and all.
//! - **How it prints.** Beside the screen colour, the same colour through the
//!   document's press, and a word when the press cannot reach it.
//! - **What uses it.** Objects, text and styles, counted and visited in turn,
//!   so "what does changing this change" has an answer before the change.

use std::ops::RangeInclusive;

use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};

use tessera_color::Color;
use tessera_color::managed::{Conversion, Proof};
use tessera_document::nodes::Swatch;

use super::style_ui;
use crate::app::{DeletingSwatch, FoundSwatchUses, StyleKind, SwatchUses, TesseraApp};
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// The built-ins, by the names the panel lists them under. Never a swatch's
/// own name: a swatch called `[Black]` would be two colours answering to
/// one name.
pub(crate) const NONE: &str = "[None]";
pub(crate) const PAPER: &str = "[Paper]";
pub(crate) const BLACK: &str = "[Black]";

/// Paper as a press means it: no ink at all, not RGB white.
pub(crate) const PAPER_INK: Color = Color::Cmyk {
    c: 0.0,
    m: 0.0,
    y: 0.0,
    k: 0.0,
    a: 1.0,
};
/// Black as a press means it: the black plate alone, not four inks.
pub(crate) const BLACK_INK: Color = Color::Cmyk {
    c: 0.0,
    m: 0.0,
    y: 0.0,
    k: 1.0,
    a: 1.0,
};

/// The window, and the question a delete asks.
pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    editor(ctx, state);
    delete_dialog(ctx, state);
}

// --- the numbers a colour is written in -------------------------------------

/// The space a process colour is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Cmyk,
    Rgb,
    Lab,
}

impl Mode {
    /// The space `colour` is written in: a spot's process stand-in's, and
    /// none for a tint, which is written as a share of another swatch.
    pub(crate) fn of(colour: &Color) -> Option<Self> {
        match colour {
            Color::Cmyk { .. } => Some(Self::Cmyk),
            Color::Rgb { .. } => Some(Self::Rgb),
            Color::Lab { .. } => Some(Self::Lab),
            Color::Spot { fallback, .. } => Self::of(fallback),
            Color::Swatch { .. } => None,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Cmyk => "CMYK",
            Self::Rgb => "RGB",
            Self::Lab => "Lab",
        }
    }
}

/// One of a colour's numbers, as the window shows it: a name, the range it
/// is typed in, and its unit.
struct Channel {
    name: &'static str,
    range: RangeInclusive<f32>,
    suffix: &'static str,
}

fn channels(mode: Mode) -> &'static [Channel] {
    const CMYK: [Channel; 4] = [
        Channel {
            name: "Cyan",
            range: 0.0..=100.0,
            suffix: "%",
        },
        Channel {
            name: "Magenta",
            range: 0.0..=100.0,
            suffix: "%",
        },
        Channel {
            name: "Yellow",
            range: 0.0..=100.0,
            suffix: "%",
        },
        Channel {
            name: "Black",
            range: 0.0..=100.0,
            suffix: "%",
        },
    ];
    const RGB: [Channel; 3] = [
        Channel {
            name: "Red",
            range: 0.0..=255.0,
            suffix: "",
        },
        Channel {
            name: "Green",
            range: 0.0..=255.0,
            suffix: "",
        },
        Channel {
            name: "Blue",
            range: 0.0..=255.0,
            suffix: "",
        },
    ];
    const LAB: [Channel; 3] = [
        Channel {
            name: "Lightness",
            range: 0.0..=100.0,
            suffix: "",
        },
        Channel {
            name: "a (green–red)",
            range: -128.0..=127.0,
            suffix: "",
        },
        Channel {
            name: "b (blue–yellow)",
            range: -128.0..=127.0,
            suffix: "",
        },
    ];
    match mode {
        Mode::Cmyk => &CMYK,
        Mode::Rgb => &RGB,
        Mode::Lab => &LAB,
    }
}

/// The process colour a swatch's colour is edited as: a spot's stand-in, or
/// the colour itself.
fn process(colour: &Color) -> &Color {
    match colour {
        Color::Spot { fallback, .. } => process(fallback),
        other => other,
    }
}

/// `colour` with its process colour replaced, keeping a spot's name and tint.
fn with_process(colour: &Color, new: Color) -> Color {
    match colour {
        Color::Spot {
            name,
            tint,
            fallback,
        } => Color::Spot {
            name: name.clone(),
            tint: *tint,
            fallback: Box::new(with_process(fallback, new)),
        },
        _ => new,
    }
}

/// A colour's numbers in the units the window shows them in: inks in
/// percent, RGB in 0–255 as every other program types it, Lab as it is.
pub(crate) fn channel_values(colour: &Color) -> Vec<f32> {
    match process(colour) {
        Color::Cmyk { c, m, y, k, .. } => vec![c * 100.0, m * 100.0, y * 100.0, k * 100.0],
        Color::Rgb { r, g, b, .. } => vec![r * 255.0, g * 255.0, b * 255.0],
        Color::Lab { l, a, b, .. } => vec![*l, *a, *b],
        _ => Vec::new(),
    }
}

/// `colour` with its `index`th number set to `value`, in the window's units.
pub(crate) fn with_channel(colour: &Color, index: usize, value: f32) -> Color {
    let changed = match process(colour).clone() {
        Color::Cmyk {
            mut c,
            mut m,
            mut y,
            mut k,
            a,
        } => {
            let v = (value / 100.0).clamp(0.0, 1.0);
            match index {
                0 => c = v,
                1 => m = v,
                2 => y = v,
                _ => k = v,
            }
            Color::Cmyk { c, m, y, k, a }
        }
        Color::Rgb {
            mut r,
            mut g,
            mut b,
            a,
        } => {
            let v = (value / 255.0).clamp(0.0, 1.0);
            match index {
                0 => r = v,
                1 => g = v,
                _ => b = v,
            }
            Color::Rgb { r, g, b, a }
        }
        Color::Lab {
            mut l,
            mut a,
            mut b,
            alpha,
        } => {
            match index {
                0 => l = value.clamp(0.0, 100.0),
                1 => a = value.clamp(-128.0, 127.0),
                _ => b = value.clamp(-128.0, 127.0),
            }
            Color::Lab { l, a, b, alpha }
        }
        other => other,
    };
    with_process(colour, changed)
}

/// The press, as far as converting a colour needs it: how it draws inks on
/// the screen, and how it separates a screen colour into inks.
#[derive(Clone, Copy, Default)]
pub(crate) struct Press<'a> {
    pub proof: Option<&'a Proof>,
    pub ink: Option<&'a Conversion>,
}

impl Press<'_> {
    /// How `colour` looks: inks as the press prints them when there is a
    /// press to say so, anything else as the screen draws it.
    fn screen(&self, colour: &Color) -> [f32; 3] {
        let [r, g, b, _] = match (colour, self.proof) {
            (Color::Cmyk { .. }, Some(proof)) if proof.converts_ink() => proof.show(colour),
            _ => colour.to_rgb_f32(),
        };
        [r, g, b]
    }

    /// The inks for a screen colour: the press's separation when there is
    /// one, the formula's otherwise. Pure black is the black plate alone,
    /// as the PDF writer makes it, and not the rich black a profile answers.
    fn inks(&self, rgb: [f32; 3]) -> [f32; 4] {
        if rgb.iter().all(|v| *v <= 0.0) {
            return [0.0, 0.0, 0.0, 1.0];
        }
        match self.ink {
            Some(ink) if ink.channels() == 4 => ink.apply(rgb).map(|v| v.clamp(0.0, 1.0)),
            _ => tessera_color::naive_cmyk(rgb),
        }
    }
}

/// The colour `colour` stands for, written in `to`: the same colour in other
/// numbers, as near as the press allows, rather than the same numbers in
/// another space. A spot keeps its name and tint; only its stand-in moves.
pub(crate) fn convert(colour: &Color, to: Mode, press: Press<'_>) -> Color {
    let source = process(colour);
    if Mode::of(source) == Some(to) {
        return colour.clone();
    }
    let alpha = source.to_rgb_f32()[3];
    let rgb = press.screen(source);
    let converted = match to {
        Mode::Rgb => Color::Rgb {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
            a: alpha,
        },
        Mode::Lab => {
            let [l, a, b] = tessera_color::srgb_to_lab(rgb);
            Color::Lab { l, a, b, alpha }
        }
        Mode::Cmyk => {
            let [c, m, y, k] = press.inks(rgb);
            Color::Cmyk {
                c,
                m,
                y,
                k,
                a: alpha,
            }
        }
    };
    with_process(colour, converted)
}

/// A colour's numbers in a line: "C 0 M 91 Y 76 K 0", "R 200 G 16 B 46",
/// "L 48 a 70 b 42", or "40% of Brand" for a tint.
pub(crate) fn numbers(colour: &Color) -> String {
    let whole = |v: f32| format!("{}", v.round() as i32);
    match process(colour) {
        Color::Swatch { name, tint } => format!("{}% of {name}", whole(tint * 100.0)),
        other => {
            let Some(mode) = Mode::of(other) else {
                return String::new();
            };
            let letters: &[&str] = match mode {
                Mode::Cmyk => &["C", "M", "Y", "K"],
                Mode::Rgb => &["R", "G", "B"],
                Mode::Lab => &["L", "a", "b"],
            };
            letters
                .iter()
                .zip(channel_values(other))
                .map(|(letter, v)| format!("{letter} {}", whole(v)))
                .collect::<Vec<_>>()
                .join("  ")
        }
    }
}

/// An RGB colour as the six hex digits a brand guide gives it in.
pub(crate) fn hex(colour: &Color) -> Option<String> {
    let Color::Rgb { r, g, b, .. } = process(colour) else {
        return None;
    };
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!("#{:02X}{:02X}{:02X}", byte(*r), byte(*g), byte(*b)))
}

/// Six hex digits, with or without the `#`, as an RGB colour.
pub(crate) fn parse_hex(text: &str) -> Option<[f32; 3]> {
    let digits = text.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?].map(|v| f32::from(v) / 255.0))
}

// --- what uses a swatch -----------------------------------------------------

/// Every swatch's uses, found again only when the document has changed.
fn found(state: &mut TesseraApp) -> &FoundSwatchUses {
    let document = state.active;
    let revision = state.active().document().revision();
    let stale = state
        .swatches_window
        .found
        .as_ref()
        .is_none_or(|found| found.document != document || found.revision != revision);
    if stale {
        let doc = state.active().document();
        let uses = doc
            .swatches
            .iter()
            .map(|swatch| SwatchUses {
                name: swatch.name.clone(),
                references: doc.swatch_references(&swatch.name),
                text: crate::find::uses_of_swatch(doc, &swatch.name),
            })
            .collect();
        state.swatches_window.found = Some(FoundSwatchUses {
            document,
            revision,
            uses,
        });
    }
    state.swatches_window.found.as_ref().expect("just found")
}

/// Where the swatch `name` is used.
pub(crate) fn uses_of(state: &mut TesseraApp, name: &str) -> SwatchUses {
    found(state)
        .uses
        .iter()
        .find(|uses| uses.name == name)
        .cloned()
        .unwrap_or_else(|| SwatchUses {
            name: name.to_owned(),
            references: Default::default(),
            text: Vec::new(),
        })
}

/// How many places use each swatch, in the panel's order.
pub(crate) fn use_counts(state: &mut TesseraApp) -> Vec<(String, usize)> {
    found(state)
        .uses
        .iter()
        .map(|uses| (uses.name.clone(), uses.places()))
        .collect()
}

impl SwatchUses {
    /// Places in the text: each stretch to go to, and a story shown in no
    /// frame — or naming it only in a footnote — as one place more.
    pub(crate) fn text_places(&self) -> usize {
        let unvisited = self
            .references
            .stories
            .iter()
            .filter(|story| !self.text.iter().any(|hit| hit.story == **story))
            .count();
        self.text.len() + unvisited
    }

    pub(crate) fn styles(&self) -> usize {
        self.references.paragraph_styles.len()
            + self.references.character_styles.len()
            + self.references.object_styles.len()
    }

    /// Every place, each counted once: what deleting it would touch.
    pub(crate) fn places(&self) -> usize {
        self.references.frames.len()
            + self.text_places()
            + self.styles()
            + self.references.swatches.len()
            + usize::from(self.references.text_default)
    }

    /// The places, in a sentence: "3 objects, 2 places in the text and
    /// 1 style".
    pub(crate) fn sentence(&self) -> String {
        let count = |n: usize, one: &str, many: &str| match n {
            0 => None,
            1 => Some(format!("1 {one}")),
            n => Some(format!("{n} {many}")),
        };
        let parts: Vec<String> = [
            count(self.references.frames.len(), "object", "objects"),
            count(
                self.text_places(),
                "place in the text",
                "places in the text",
            ),
            count(self.styles(), "style", "styles"),
            count(self.references.swatches.len(), "tint", "tints"),
            self.references
                .text_default
                .then(|| "the default text colour".to_string()),
        ]
        .into_iter()
        .flatten()
        .collect();
        match parts.as_slice() {
            [] => "Nothing uses it.".to_string(),
            [one] => format!("Used by {one}."),
            [rest @ .., last] => format!("Used by {} and {last}.", rest.join(", ")),
        }
    }

    /// The places Next and Previous visit, in reading order: the objects,
    /// then the text.
    fn visits(&self) -> Vec<Visit> {
        self.references
            .frames
            .iter()
            .map(|frame| Visit::Object(*frame))
            .chain(self.text.iter().cloned().map(Visit::Text))
            .collect()
    }
}

enum Visit {
    Object(tessera_document::ids::FrameId),
    Text(crate::find::Hit),
}

// --- changing a swatch ------------------------------------------------------

/// Put the edited swatch in place of `old`.
///
/// `dragging` says the change came from a slider or number being dragged.
/// The first change of a drag records an undo step as every edit does; the
/// rest of that drag change the document under the same step, so the page
/// follows the drag and one undo takes it back whole.
fn edit(state: &mut TesseraApp, old: &str, swatch: Swatch, dragging: bool) {
    let continuing = dragging && state.swatches_window.dragging.as_deref() == Some(old);
    state.swatches_window.dragging = dragging.then(|| swatch.name.clone());
    if state.swatches_window.chosen.as_deref() == Some(old) {
        state.swatches_window.chosen = Some(swatch.name.clone());
    }
    if continuing {
        let open = state.active_mut();
        // undo-bracketed: the press that began this drag recorded its entry
        // through `Command::EditSwatch`; the drag's later frames follow it.
        if open.document_mut().edit_swatch(old, swatch) {
            open.dirty = true;
        }
    } else {
        apply(
            state,
            Command::EditSwatch {
                old: old.to_owned(),
                swatch,
            },
        );
    }
}

/// Why `draft` cannot be the name of the swatch now called `current`, if
/// it cannot.
pub(crate) fn name_problem(
    state: &TesseraApp,
    current: Option<&str>,
    draft: &str,
) -> Option<&'static str> {
    let draft = draft.trim();
    if draft.is_empty() {
        return Some("A swatch needs a name.");
    }
    if [NONE, PAPER, BLACK].contains(&draft) {
        return Some("[None], [Paper] and [Black] are the built-in colours' names.");
    }
    if Some(draft) != current && state.active().document().swatch(draft).is_some() {
        return Some("Another swatch is already called that.");
    }
    None
}

/// "Brand 2", or the first such name free.
pub(crate) fn unused_name(state: &TesseraApp, stem: &str) -> String {
    let doc = state.active().document();
    if doc.swatch(stem).is_none() && name_problem(state, None, stem).is_none() {
        return stem.to_owned();
    }
    (2..)
        .map(|n| format!("{stem} {n}"))
        .find(|name| doc.swatch(name).is_none())
        .expect("a free name")
}

/// Make a tint swatch of `base` at `tint`, and open it.
pub(crate) fn new_tint(state: &mut TesseraApp, base: &str, tint: f32) {
    let percent = (tint * 100.0).round() as i32;
    let name = unused_name(state, &format!("{base} {percent}%"));
    apply(
        state,
        Command::SetSwatch(Swatch::new(
            name.clone(),
            Color::Swatch {
                name: base.to_owned(),
                tint,
            },
        )),
    );
    state.swatches_window.chosen = Some(name);
}

/// Delete a swatch: straight away when nothing uses it, and otherwise by
/// asking what its uses should become.
pub(crate) fn delete(state: &mut TesseraApp, name: &str) {
    if uses_of(state, name).places() == 0 {
        apply(
            state,
            Command::RemoveSwatch {
                name: name.to_owned(),
            },
        );
        forget(state, name);
    } else {
        state.swatches_window.deleting = Some(DeletingSwatch {
            name: name.to_owned(),
            with: None,
        });
    }
}

/// Stop pointing at a swatch that has gone.
fn forget(state: &mut TesseraApp, name: &str) {
    let window = &mut state.swatches_window;
    if window.chosen.as_deref() == Some(name) {
        window.chosen = None;
        window.editing = false;
    }
}

/// The question a delete asks when the swatch is in use: what its uses keep.
///
/// Two answers, InDesign's two. The third — leave them naming a colour that
/// no longer exists — is what the panel used to do without asking, and it
/// is never what anybody means: every use turned the alarming magenta.
fn delete_dialog(ctx: &egui::Context, state: &mut TesseraApp) {
    let Some(mut deleting) = state.swatches_window.deleting.clone() else {
        return;
    };
    let uses = uses_of(state, &deleting.name);
    let others: Vec<String> = state
        .active()
        .document()
        .swatches
        .iter()
        .map(|s| s.name.clone())
        .filter(|n| *n != deleting.name && !uses.references.swatches.contains(n))
        .collect();
    let mut go = false;
    let mut cancel = false;
    let response = egui::Modal::new(egui::Id::new("delete-swatch"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 440.0));
            ui.heading(format!("Delete “{}”?", deleting.name));
            ui.colored_label(Theme::text_muted(), uses.sentence());
            ui.add_space(Theme::space_2());
            let mut keep = deleting.with.is_none();
            if ui
                .radio_value(&mut keep, true, "Keep its colour in each use")
                .on_hover_text("Each use keeps what it looks like now, as a colour of its own")
                .changed()
            {
                deleting.with = None;
            }
            ui.add_enabled_ui(!others.is_empty(), |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .radio_value(&mut keep, false, "Use another swatch instead")
                        .changed()
                    {
                        deleting.with = others.first().cloned();
                    }
                    if !keep {
                        let mut with = deleting.with.clone().unwrap_or_default();
                        crate::icons::reads_as(
                            egui::ComboBox::from_id_salt("delete-swatch-with")
                                .selected_text(&with)
                                .show_ui(ui, |ui| {
                                    for other in &others {
                                        ui.selectable_value(&mut with, other.clone(), other);
                                    }
                                })
                                .response,
                            "Replacement swatch",
                            egui::WidgetType::ComboBox,
                            None,
                        );
                        deleting.with = Some(with);
                    }
                });
            });
            ui.add_space(Theme::space_3());
            ui.horizontal(|ui| {
                go = ui.add(super::primary_button("Delete")).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
    if response.should_close() || cancel {
        state.swatches_window.deleting = None;
        return;
    }
    if go {
        state.swatches_window.deleting = None;
        apply(
            state,
            Command::ReplaceSwatch {
                name: deleting.name.clone(),
                with: deleting.with.clone(),
            },
        );
        forget(state, &deleting.name);
        return;
    }
    state.swatches_window.deleting = Some(deleting);
}

// --- the window -------------------------------------------------------------

/// How far the window's corners round, as the style window's do.
const WINDOW_RADIUS: u8 = 12;
/// The height kept for the footer's actions under the scrolling cards.
const FOOTER: f32 = 52.0;

fn editor(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.swatches_window.editing {
        return;
    }
    let swatch = state
        .swatches_window
        .chosen
        .as_deref()
        .and_then(|name| state.active().document().swatch(name))
        .cloned();
    let Some(swatch) = swatch else {
        // Deleted, undone away, or a built-in: nothing to edit.
        state.swatches_window.editing = false;
        return;
    };
    // A drag ends when the button comes up, wherever the pointer is.
    if !ctx.input(|i| i.pointer.any_down()) {
        state.swatches_window.dragging = None;
    }
    let mut open = true;
    let frame = egui::Frame::window(&ctx.style_of(ctx.theme()))
        .fill(Theme::panel_bg())
        .stroke(Stroke::new(1.0, Theme::border()))
        .corner_radius(WINDOW_RADIUS)
        .inner_margin(0);
    egui::Window::new(format!("Swatch: {}", swatch.name))
        // By id, not title: the title changes with every rename.
        .id(egui::Id::new("swatch-editor"))
        .title_bar(false)
        .frame(frame)
        .resizable(true)
        // As tall as the screen allows, to a point: the cards read top to
        // bottom, and a window that scrolls on a screen with room to spare
        // hides what uses the colour under the fold.
        .default_size([
            560.0,
            (ctx.content_rect().height() - 48.0).clamp(420.0, 820.0),
        ])
        .min_size([480.0, 420.0])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = Theme::space_2();
            header(ui, state, &swatch, &mut open);
            let rule = ui.min_rect().bottom();
            ui.painter().hline(
                ui.max_rect().x_range(),
                rule,
                Stroke::new(1.0, Theme::rule()),
            );
            let body = (ui.available_height() - FOOTER).max(120.0);
            egui::ScrollArea::vertical()
                .id_salt("swatch-editor-body")
                .max_height(body)
                .min_scrolled_height(body)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(18, 14))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            colour_card(ui, state, &swatch);
                            preview_card(ui, state, &swatch);
                            uses_card(ui, state, &swatch);
                        });
                });
            footer(ui, state, &swatch);
        });
    if !open {
        state.swatches_window.editing = false;
    }
}

/// The top of the window: the colour, large; what kind of swatch it is;
/// its name, which is typed over in place; and its numbers.
fn header(ui: &mut Ui, state: &mut TesseraApp, swatch: &Swatch, open: &mut bool) {
    let doc = state.active().document();
    let shown = doc
        .resolve_colour(&Color::Swatch {
            name: swatch.name.clone(),
            tint: 1.0,
        })
        .to_rgb_f32();
    let kind = kind_label(swatch);
    let figures = match hex(&swatch.colour) {
        Some(code) => format!("{}  ·  {code}", numbers(&swatch.colour)),
        None => numbers(&swatch.colour),
    };
    let mut rename: Option<String> = None;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 18,
            right: 12,
            top: 14,
            bottom: 14,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(52.0), Sense::hover());
                chip(ui.painter(), tile, shown, 10.0, is_spot(swatch));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    style_ui::overline(ui, &kind);
                    rename = name_field(ui, state, &swatch.name);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&figures)
                                .size(Theme::TYPE_SM)
                                .color(Theme::text_muted()),
                        )
                        .truncate()
                        .selectable(false),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if super::panels::icon_button(ui, Icon::Close, "Close", false) {
                        *open = false;
                    }
                });
            });
        });
    if let Some(name) = rename {
        edit(
            state,
            &swatch.name,
            Swatch {
                name,
                ..swatch.clone()
            },
            false,
        );
    }
}

/// "Process colour · CMYK", "Spot colour · Lab", "Tint".
fn kind_label(swatch: &Swatch) -> String {
    match (Mode::of(&swatch.colour), is_spot(swatch)) {
        (None, _) => "Tint".to_string(),
        (Some(mode), true) => format!("Spot colour · {}", mode.label()),
        (Some(mode), false) => format!("Process colour · {}", mode.label()),
    }
}

/// Whether a swatch prints on a plate of its own.
pub(crate) fn is_spot(swatch: &Swatch) -> bool {
    swatch.spot || matches!(swatch.colour, Color::Spot { .. })
}

/// The name, as a heading that is a field: typed over in place, kept when
/// the field is left, and put back when what was typed cannot be a name.
fn name_field(ui: &mut Ui, state: &TesseraApp, current: &str) -> Option<String> {
    let id = egui::Id::new("swatch-editor-name");
    let focused = ui.memory(|m| m.has_focus(id));
    let mut draft = if focused {
        ui.data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| current.to_owned())
    } else {
        current.to_owned()
    };
    let response = ui
        .scope(|ui| {
            // A heading until it is pointed at: no ground and no edge, so the
            // name reads as the window's title, which it is.
            let visuals = ui.visuals_mut();
            visuals.text_edit_bg_color = Some(Color32::TRANSPARENT);
            visuals.widgets.inactive.bg_stroke = Stroke::NONE;
            visuals.widgets.inactive.corner_radius = 6.into();
            visuals.widgets.hovered.corner_radius = 6.into();
            // No margin at the left, so the name's first letter stands under
            // the caption's rather than a margin's width in from it.
            ui.add(
                egui::TextEdit::singleline(&mut draft)
                    .id(id)
                    .font(style_ui::heading_font(20.0))
                    .text_color(Theme::text_primary())
                    .desired_width((ui.available_width() - 60.0).max(160.0))
                    .margin(egui::Margin {
                        left: 0,
                        right: 4,
                        top: 1,
                        bottom: 1,
                    }),
            )
        })
        .inner;
    let response = crate::icons::speak_as(response, "Swatch name");
    ui.data_mut(|d| d.insert_temp(id, draft.clone()));
    if response.has_focus()
        && let Some(problem) = name_problem(state, Some(current), &draft)
    {
        ui.colored_label(Theme::error(), problem);
    }
    (response.lost_focus()
        && draft.trim() != current
        && name_problem(state, Some(current), &draft).is_none())
    .then(|| draft.trim().to_owned())
}

/// A colour in a rounded block, on the panel's own ground so a colour with
/// alpha is not shown over whatever is behind it; a spot marked with the
/// dot InDesign marks one with.
pub(crate) fn chip(painter: &egui::Painter, rect: Rect, rgba: [f32; 4], radius: f32, spot: bool) {
    painter.rect_filled(rect, radius, Theme::panel_bg_solid());
    painter.rect_filled(rect, radius, style_ui::srgb(rgba));
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Theme::border()),
        egui::StrokeKind::Inside,
    );
    if spot {
        let r = (rect.height() * 0.14).clamp(2.5, 6.0);
        let centre = rect.right_bottom() - Vec2::splat(r + 3.0);
        painter.circle(
            centre,
            r,
            Color32::WHITE,
            Stroke::new(1.0, Color32::from_black_alpha(140)),
        );
        painter.circle_filled(centre, r * 0.45, Color32::from_black_alpha(200));
    }
}

/// The colour itself: process or spot, the space it is written in, and a
/// slider for each of its numbers — or, for a tint, what it is a tint of.
fn colour_card(ui: &mut Ui, state: &mut TesseraApp, swatch: &Swatch) {
    let mut edited = swatch.clone();
    let mut dragging = false;
    let mut changed = false;

    if let Color::Swatch { name: base, tint } = &swatch.colour {
        let bases: Vec<String> = state
            .active()
            .document()
            .swatches
            .iter()
            .filter(|s| s.name != swatch.name && !names(&s.colour, &swatch.name))
            .map(|s| s.name.clone())
            .collect();
        let base_rgba = state
            .active()
            .document()
            .resolve_colour(&Color::Swatch {
                name: base.clone(),
                tint: 1.0,
            })
            .to_rgb_f32();
        style_ui::card(ui, Some("Tint"), |ui| {
            style_ui::row(ui, |ui| {
                style_ui::name_cell(ui, "Tint of", true);
                let mut chosen = base.clone();
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt("swatch-tint-of")
                        .selected_text(&chosen)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for other in &bases {
                                ui.selectable_value(&mut chosen, other.clone(), other);
                            }
                        })
                        .response,
                    "Tint of",
                    egui::WidgetType::ComboBox,
                    None,
                );
                if chosen != *base {
                    edited.colour = Color::Swatch {
                        name: chosen,
                        tint: *tint,
                    };
                    changed = true;
                }
            });
            style_ui::row(ui, |ui| {
                style_ui::name_cell(ui, "Tint", true);
                let mut percent = tint * 100.0;
                let paint = |v: f32| {
                    let t = v / 100.0;
                    let [r, g, b, a] = base_rgba;
                    [
                        1.0 - t * (1.0 - r),
                        1.0 - t * (1.0 - g),
                        1.0 - t * (1.0 - b),
                        a,
                    ]
                };
                let (moved, drag) = channel(ui, "Tint", &mut percent, 0.0..=100.0, "%", paint);
                if moved {
                    edited.colour = Color::Swatch {
                        name: base.clone(),
                        tint: (percent / 100.0).clamp(0.0, 1.0),
                    };
                    changed = true;
                    dragging |= drag;
                }
            });
            super::panel_ui::hint(
                ui,
                "A tint is a share of its swatch, and follows it: edit the swatch and every \
                 tint of it changes too.",
            );
        });
    } else if let Some(mode) = Mode::of(&swatch.colour) {
        let intent = state.active().document().output_intent.clone();
        let (proof, ink) = state.soft_proof.press_and_ink(intent.as_ref());
        let press = Press { proof, ink };
        style_ui::card(ui, Some("Colour"), |ui| {
            style_ui::row(ui, |ui| {
                style_ui::name_cell(ui, "Ink", true);
                let mut spot = is_spot(swatch);
                if style_ui::segmented(
                    ui,
                    "Ink",
                    &mut spot,
                    &[
                        (style_ui::Segment::Text("Process"), false),
                        (style_ui::Segment::Text("Spot"), true),
                    ],
                    false,
                ) {
                    edited.spot = spot;
                    if !spot {
                        // A spot written as one keeps only its stand-in.
                        edited.colour = process(&swatch.colour).clone();
                    }
                    changed = true;
                }
                ui.add_space(8.0);
                ui.colored_label(
                    Theme::text_muted(),
                    if spot {
                        "prints on a plate of its own"
                    } else {
                        "mixed from the four process inks"
                    },
                );
            });
            style_ui::row(ui, |ui| {
                style_ui::name_cell(ui, "Mode", true);
                let mut to = mode;
                if style_ui::segmented(
                    ui,
                    "Mode",
                    &mut to,
                    &[
                        (style_ui::Segment::Text("CMYK"), Mode::Cmyk),
                        (style_ui::Segment::Text("RGB"), Mode::Rgb),
                        (style_ui::Segment::Text("Lab"), Mode::Lab),
                    ],
                    false,
                ) {
                    edited.colour = convert(&swatch.colour, to, press);
                    changed = true;
                }
                // What switching to inks would do, said before it is done:
                // a conversion is a decision about the colour, and the one
                // through the press is not the one by the formula.
                if to != Mode::Cmyk {
                    ui.add_space(8.0);
                    ui.colored_label(
                        Theme::text_muted(),
                        if press.ink.is_some() {
                            "CMYK separates it for the press"
                        } else {
                            "CMYK converts by formula: no press chosen"
                        },
                    );
                }
            });
            ui.add_space(4.0);
            let values = channel_values(&swatch.colour);
            for (index, (spec, value)) in channels(mode).iter().zip(values).enumerate() {
                style_ui::row(ui, |ui| {
                    style_ui::name_cell(ui, spec.name, true);
                    let mut v = value;
                    let base = swatch.colour.clone();
                    let paint = |x: f32| press_free_rgba(&with_channel(&base, index, x));
                    let (moved, drag) = channel(
                        ui,
                        spec.name,
                        &mut v,
                        spec.range.clone(),
                        spec.suffix,
                        paint,
                    );
                    if moved {
                        edited.colour = with_channel(&swatch.colour, index, v);
                        changed = true;
                        dragging |= drag;
                    }
                });
            }
            if let Some(code) = hex(&swatch.colour) {
                style_ui::row(ui, |ui| {
                    style_ui::name_cell(ui, "Hex", true);
                    if let Some(rgb) = hex_field(ui, &code) {
                        edited.colour = with_process(
                            &swatch.colour,
                            Color::Rgb {
                                r: rgb[0],
                                g: rgb[1],
                                b: rgb[2],
                                a: process(&swatch.colour).to_rgb_f32()[3],
                            },
                        );
                        changed = true;
                    }
                });
            }
        });
    }
    if changed {
        edit(state, &swatch.name, edited, dragging);
    }
}

/// Whether `colour` names the swatch `name`.
fn names(colour: &Color, name: &str) -> bool {
    matches!(colour, Color::Swatch { name: n, .. } if n == name)
}

/// What a colour looks like on the screen, by the formula: a slider's track
/// is repainted every frame, and a press transform per step of it would be
/// thirty calls into Little CMS a frame for a hint of a gradient.
fn press_free_rgba(colour: &Color) -> [f32; 4] {
    process(colour).to_rgb_f32()
}

/// One of a colour's numbers: a track painted with what the colour becomes
/// along it, and the number itself, typed or dragged.
///
/// Answers whether the value moved, and whether it moved by the pointer
/// held down on it — a press and the drag that may follow it are one
/// gesture, and one undo step — rather than by a key or a typed number.
fn channel(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    suffix: &str,
    paint: impl Fn(f32) -> [f32; 4],
) -> (bool, bool) {
    let (start, end) = (*range.start(), *range.end());
    let width = (ui.available_width() - style_ui::NUMBER_WIDTH - 16.0).clamp(120.0, 280.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::click_and_drag());
    let track = Rect::from_center_size(rect.center(), Vec2::new(width - 16.0, 10.0));
    let painter = ui.painter();

    // The track: the colour at every step of this number, the others held.
    painter.rect_filled(track, 3.0, Theme::panel_bg_solid());
    let steps = 32;
    let mut mesh = egui::Mesh::default();
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = egui::lerp(track.x_range(), t);
        let colour = style_ui::srgb(paint(egui::lerp(start..=end, t)));
        mesh.colored_vertex(egui::pos2(x, track.top()), colour);
        mesh.colored_vertex(egui::pos2(x, track.bottom()), colour);
        if i > 0 {
            let at = 2 * i;
            mesh.add_triangle(at - 2, at - 1, at);
            mesh.add_triangle(at - 1, at, at + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.rect_stroke(
        track,
        3.0,
        Stroke::new(1.0, Theme::border()),
        egui::StrokeKind::Outside,
    );

    let mut moved = false;
    let mut by_drag = false;
    if response.is_pointer_button_down_on()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let at = egui::lerp(start..=end, t).round();
        if at != *value {
            *value = at;
            moved = true;
            // The press as well as the drag: the press is where the drag
            // starts, and recording it alone made the drag a second step.
            by_drag = true;
        }
    }
    if response.has_focus() {
        let step = ui.input(|i| {
            let big = if i.modifiers.shift { 10.0 } else { 1.0 };
            if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp) {
                big
            } else if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown) {
                -big
            } else {
                0.0
            }
        });
        if step != 0.0 {
            *value = (value.round() + step).clamp(start, end);
            moved = true;
        }
    }

    // The knob, filled with the colour as it is now.
    let t = ((*value - start) / (end - start)).clamp(0.0, 1.0);
    let knob = egui::pos2(egui::lerp(track.x_range(), t), track.center().y);
    painter.circle(
        knob,
        8.0,
        style_ui::srgb(paint(*value)),
        Stroke::new(2.0, Color32::WHITE),
    );
    painter.circle_stroke(knob, 9.0, Stroke::new(1.0, Color32::from_black_alpha(110)));
    if response.has_focus() {
        painter.circle_stroke(knob, 12.0, Stroke::new(1.0, Theme::focus()));
    }
    crate::icons::reads_as(response, label, egui::WidgetType::Slider, None);

    ui.add_space(8.0);
    let number = ui.add_sized(
        [style_ui::NUMBER_WIDTH - 24.0, 22.0],
        egui::DragValue::new(value)
            .range(range)
            .speed(0.5)
            .fixed_decimals(0)
            .suffix(suffix),
    );
    let number = crate::icons::speak_as(number, &format!("{label} value"));
    if number.changed() {
        moved = true;
        by_drag |= number.dragged();
    }
    (moved, by_drag)
}

/// Six hex digits to type an RGB colour as, answered when a valid code is
/// committed.
fn hex_field(ui: &mut Ui, code: &str) -> Option<[f32; 3]> {
    let id = egui::Id::new("swatch-editor-hex");
    let focused = ui.memory(|m| m.has_focus(id));
    let mut draft = if focused {
        ui.data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| code.to_owned())
    } else {
        code.to_owned()
    };
    let response = ui.add(
        egui::TextEdit::singleline(&mut draft)
            .id(id)
            .font(egui::TextStyle::Monospace)
            .desired_width(96.0)
            .min_size(Vec2::new(96.0, 22.0))
            .vertical_align(egui::Align::Center),
    );
    let response = crate::icons::speak_as(response, "Hex");
    ui.data_mut(|d| d.insert_temp(id, draft.clone()));
    let parsed = parse_hex(&draft);
    if focused && parsed.is_none() {
        ui.colored_label(Theme::error(), "six hex digits, as #C8102E");
    }
    (response.lost_focus() && !draft.trim().eq_ignore_ascii_case(code))
        .then_some(parsed)
        .flatten()
}

/// The colour as the screen draws it and as the document's press will
/// print it, side by side, with a word when the two are far apart; and its
/// tints, any of which can be made a swatch of its own.
fn preview_card(ui: &mut Ui, state: &mut TesseraApp, swatch: &Swatch) {
    let doc = state.active().document();
    let reference = |tint: f32| Color::Swatch {
        name: swatch.name.clone(),
        tint,
    };
    let resolved = doc.resolve_colour(&reference(1.0));
    let screen = resolved.to_rgb_f32();
    let intent = doc.output_intent.clone();
    let tints: Vec<(f32, Color)> = [1.0, 0.8, 0.6, 0.4, 0.2]
        .into_iter()
        .map(|t| (t, doc.resolve_colour(&reference(t))))
        .collect();
    let written_in = Mode::of(&swatch.colour).or_else(|| {
        // A tint is written in its base's space.
        let mut at = swatch.colour.clone();
        for _ in 0..8 {
            let Color::Swatch { name, .. } = &at else {
                break;
            };
            at = doc.swatch(name)?.colour.clone();
        }
        Mode::of(&at)
    });
    let press_name = intent.as_ref().map(|i| i.description.clone());
    let trouble = state.soft_proof.trouble.clone();
    let proof = state.soft_proof.press(intent.as_ref());
    let printed = proof.map(|p| p.show(&resolved));
    let converts_ink = proof.is_some_and(|p| p.converts_ink());
    let ramp: Vec<(f32, [f32; 4])> = tints
        .iter()
        .map(|(t, c)| (*t, proof.map_or_else(|| c.to_rgb_f32(), |p| p.show(c))))
        .collect();
    let mut make_tint = None;

    style_ui::card(ui, Some("How it looks"), |ui| {
        let gap = 12.0;
        let half = ((ui.available_width() - gap) / 2.0).max(80.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            swatch_block(ui, half, screen, "On screen", None);
            match (printed, &press_name) {
                (Some(printed), Some(press)) => {
                    swatch_block(ui, half, printed, "Printed", Some(press));
                }
                _ => {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(half, 96.0), Sense::hover());
                    ui.painter().rect(
                        Rect::from_min_size(rect.min, Vec2::new(half, 64.0)),
                        8.0,
                        Theme::panel_bg_solid(),
                        Stroke::new(1.0, Theme::rule()),
                        egui::StrokeKind::Inside,
                    );
                    let why = match &trouble {
                        Some(trouble) if press_name.is_some() => {
                            format!("The press profile could not be used: {trouble}")
                        }
                        _ => "Choose a press in Document setup › Colour management to see \
                              how it prints."
                            .to_string(),
                    };
                    let galley = ui.painter().layout(
                        why,
                        egui::FontId::proportional(Theme::TYPE_SM),
                        Theme::text_muted(),
                        half - 20.0,
                    );
                    ui.painter().galley(
                        rect.min + Vec2::new(10.0, 32.0 - galley.size().y / 2.0),
                        galley,
                        Theme::text_muted(),
                    );
                }
            }
        });
        // Whether the press can reach it. Only a colour written in some
        // other space than the press's own can be out of reach: inks are
        // what the press prints, and the difference there is the screen's.
        if let Some(printed) = printed
            && !(written_in == Some(Mode::Cmyk) && converts_ink)
        {
            let apart = delta_e(screen, printed);
            if apart > 5.0 {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Theme::error(), "Outside what this press can print.");
                    ui.colored_label(
                        Theme::text_muted(),
                        format!("It will print as the right-hand block, ΔE {apart:.0} away."),
                    );
                });
            } else {
                ui.colored_label(Theme::text_muted(), "Within what this press can print.");
            }
        }
        // Tints of a tint would be a ramp whose "100%" is somebody else's
        // 40%: a tint's tints are made from its swatch.
        let tintable = !matches!(swatch.colour, Color::Swatch { .. });
        if !tintable {
            return;
        }
        ui.add_space(6.0);
        style_ui::overline(ui, "Tints");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let width = ((ui.available_width() - 4.0 * 6.0) / 5.0).max(36.0);
            for (tint, rgba) in &ramp {
                let percent = (tint * 100.0).round() as i32;
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(width, 46.0),
                    if *tint < 1.0 {
                        Sense::click()
                    } else {
                        Sense::hover()
                    },
                );
                let block = Rect::from_min_size(rect.min, Vec2::new(width, 28.0));
                chip(ui.painter(), block, *rgba, 6.0, false);
                if response.hovered() && *tint < 1.0 {
                    ui.painter().rect_stroke(
                        block,
                        6.0,
                        Stroke::new(2.0, Theme::accent()),
                        egui::StrokeKind::Outside,
                    );
                }
                ui.painter().text(
                    egui::pos2(block.center().x, block.bottom() + 9.0),
                    egui::Align2::CENTER_CENTER,
                    format!("{percent}%"),
                    egui::FontId::proportional(Theme::TYPE_SM),
                    Theme::text_muted(),
                );
                if *tint < 1.0 {
                    let label = format!("Make a {percent}% tint swatch");
                    let response =
                        crate::icons::reads_as(response, &label, egui::WidgetType::Button, None)
                            .on_hover_text(format!("{label} of {}", swatch.name));
                    if response.clicked() {
                        make_tint = Some(*tint);
                    }
                }
            }
        });
    });
    if let Some(tint) = make_tint {
        new_tint(state, &swatch.name, tint);
    }
}

/// A colour in a large block with what it is under it.
fn swatch_block(ui: &mut Ui, width: f32, rgba: [f32; 4], title: &str, detail: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 96.0), Sense::hover());
    let block = Rect::from_min_size(rect.min, Vec2::new(width, 64.0));
    chip(ui.painter(), block, rgba, 8.0, false);
    let painter = ui.painter();
    painter.text(
        egui::pos2(block.left(), block.bottom() + 6.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(Theme::TYPE_MD),
        Theme::text_primary(),
    );
    if let Some(detail) = detail {
        let mut job = egui::text::LayoutJob::simple_singleline(
            detail.to_owned(),
            egui::FontId::proportional(Theme::TYPE_SM),
            Theme::text_muted(),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(width);
        let galley = painter.layout_job(job);
        painter.galley(
            egui::pos2(block.left(), block.bottom() + 20.0),
            galley,
            Theme::text_muted(),
        );
    }
}

/// How far apart two screen colours look: CIE76 ΔE, where about 2 is the
/// least a trained eye sees and 5 is plainly a different colour.
fn delta_e(a: [f32; 4], b: [f32; 4]) -> f32 {
    let a = tessera_color::srgb_to_lab([a[0], a[1], a[2]]);
    let b = tessera_color::srgb_to_lab([b[0], b[1], b[2]]);
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}

/// What changing the swatch changes: how many objects, places in the text
/// and styles name it, with a way to visit each object and stretch of text
/// in turn, and each style and tint to open.
fn uses_card(ui: &mut Ui, state: &mut TesseraApp, swatch: &Swatch) {
    let uses = uses_of(state, &swatch.name);
    let visits = uses.visits();
    let count = visits.len();
    let at = state
        .swatches_window
        .visited
        .as_ref()
        .filter(|(name, i)| *name == swatch.name && *i < count)
        .map(|(_, i)| *i);
    let doc = state.active().document();
    let mut styles: Vec<(Icon, String, StyleTarget)> = Vec::new();
    for id in &uses.references.paragraph_styles {
        if let Some(style) = doc.paragraph_styles.get(*id) {
            styles.push((
                Icon::Pilcrow,
                style.name.clone(),
                StyleTarget::Paragraph(*id),
            ));
        }
    }
    for id in &uses.references.character_styles {
        if let Some(style) = doc.character_styles.get(*id) {
            styles.push((
                Icon::CaseSensitive,
                style.name.clone(),
                StyleTarget::Character(*id),
            ));
        }
    }
    for id in &uses.references.object_styles {
        if let Some(style) = doc.object_styles.get(*id) {
            styles.push((
                Icon::Rectangle,
                style.name.clone(),
                StyleTarget::Object(*id),
            ));
        }
    }
    let mut go: Option<usize> = None;
    let mut open_style = None;
    let mut open_tint = None;
    style_ui::card_with_action(
        ui,
        "In this document",
        |ui| {
            ui.add_enabled_ui(count > 0, |ui| {
                if super::panels::icon_button(ui, Icon::ChevronRight, "Next use", false) {
                    go = Some(at.map_or(0, |i| (i + 1) % count));
                }
                if super::panels::icon_button(ui, Icon::ChevronLeft, "Previous use", false) {
                    go = Some(at.map_or(count - 1, |i| (i + count - 1) % count));
                }
            });
            if let Some(i) = at {
                ui.colored_label(Theme::text_muted(), format!("{} of {count}", i + 1));
            }
        },
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 28.0;
                let objects = uses.references.frames.len();
                style_ui::stat(ui, objects, if objects == 1 { "object" } else { "objects" });
                style_ui::stat(ui, uses.text_places(), "in the text");
                style_ui::stat(
                    ui,
                    uses.styles(),
                    if uses.styles() == 1 {
                        "style"
                    } else {
                        "styles"
                    },
                );
                let tints = uses.references.swatches.len();
                style_ui::stat(ui, tints, if tints == 1 { "tint" } else { "tints" });
            });
            if !styles.is_empty()
                || !uses.references.swatches.is_empty()
                || uses.references.text_default
            {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(6.0);
                    for (icon, name, target) in &styles {
                        if link_tag(ui, *icon, name)
                            .on_hover_text(format!("Edit the style {name}"))
                            .clicked()
                        {
                            open_style = Some(*target);
                        }
                    }
                    for tint in &uses.references.swatches {
                        if link_tag(ui, Icon::Swatches, tint)
                            .on_hover_text(format!("Open the tint {tint}"))
                            .clicked()
                        {
                            open_tint = Some(tint.clone());
                        }
                    }
                    if uses.references.text_default {
                        style_ui::tag(ui, "Default text colour");
                    }
                });
            }
            if uses.places() == 0 {
                super::panel_ui::hint(ui, "Nothing in the document uses it yet.");
            }
        },
    );
    if let Some(i) = go {
        state.swatches_window.visited = Some((swatch.name.clone(), i));
        match &visits[i] {
            Visit::Object(frame) => super::styles::reveal_object(state, *frame),
            Visit::Text(hit) => crate::view::find::reveal(state, hit),
        }
    }
    if let Some(target) = open_style {
        let window = &mut state.styles_window;
        match target {
            StyleTarget::Paragraph(id) => {
                window.kind = StyleKind::Paragraph;
                window.paragraph = Some(id);
            }
            StyleTarget::Character(id) => {
                window.kind = StyleKind::Character;
                window.character = Some(id);
            }
            StyleTarget::Object(id) => {
                window.kind = StyleKind::Object;
                window.object = Some(id);
            }
        }
        window.editing = true;
    }
    if let Some(tint) = open_tint {
        state.swatches_window.chosen = Some(tint);
    }
}

#[derive(Clone, Copy)]
enum StyleTarget {
    Paragraph(tessera_text::story::ParagraphStyleId),
    Character(tessera_text::story::CharacterStyleId),
    Object(tessera_document::ids::ObjectStyleId),
}

/// A pill with an icon and a name that opens what it names.
fn link_tag(ui: &mut Ui, icon: Icon, text: &str) -> egui::Response {
    let font = egui::FontId::proportional(Theme::TYPE_SM);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Theme::text_primary());
    let size = Vec2::new(galley.size().x + 36.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    painter.rect(
        rect,
        12.0,
        if response.hovered() {
            Theme::hover_bg()
        } else {
            Theme::selected_bg()
        },
        Stroke::new(
            1.0,
            if response.hovered() {
                Theme::accent_edge()
            } else {
                Color32::TRANSPARENT
            },
        ),
        egui::StrokeKind::Inside,
    );
    let glyph = Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Vec2::splat(Theme::ICON_SIZE - 2.0),
    );
    crate::icons::paint(painter, glyph, icon, Theme::text_muted());
    painter.galley(
        egui::pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
        galley,
        Theme::text_primary(),
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            12.0,
            Stroke::new(1.0, Theme::focus()),
            egui::StrokeKind::Inside,
        );
    }
    crate::icons::reads_as(response, text, egui::WidgetType::Link, None)
}

/// The bottom of the window: what can be done with the swatch as a whole.
fn footer(ui: &mut Ui, state: &mut TesseraApp, swatch: &Swatch) {
    let rule = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        rule,
        Stroke::new(1.0, Theme::rule()),
    );
    let mut remove = false;
    let mut duplicate = false;
    let mut apply_now = false;
    let entry = super::swatches::Entry::Named(swatch.name.clone());
    let ready =
        super::swatches::apply_commands(state, &entry, state.swatches_window.target).is_some();
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                remove = super::panel_ui::action(ui, Icon::Trash, "Delete…")
                    .on_hover_text("Delete this swatch, choosing what its uses keep")
                    .clicked();
                duplicate = super::panel_ui::action(ui, Icon::Duplicate, "Duplicate")
                    .on_hover_text("A new swatch of the same colour, to vary")
                    .clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    apply_now = ui
                        .add_enabled(
                            ready,
                            super::primary_button("Apply")
                                .min_size(Vec2::new(0.0, 26.0))
                                .corner_radius(6),
                        )
                        .on_hover_text("Colour the selection with this swatch")
                        .on_disabled_hover_text(super::swatches::apply_hint(
                            state.swatches_window.target,
                        ))
                        .clicked();
                    super::swatches::target_choice(ui, state);
                });
            });
        });
    if remove {
        delete(state, &swatch.name);
    }
    if duplicate {
        let name = unused_name(state, &format!("{} copy", swatch.name));
        apply(
            state,
            Command::SetSwatch(Swatch {
                name: name.clone(),
                ..swatch.clone()
            }),
        );
        state.swatches_window.chosen = Some(name);
    }
    if apply_now {
        super::swatches::apply_entry(state, &entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::paint::Paint;

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 1000.0),
            )),
            events,
            ..Default::default()
        }
    }

    /// One frame of the window and its question, answering with what a
    /// screen reader would be told.
    fn draw(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::accesskit::Role, Rect)> {
        let output = crate::headless_frame::frame(ctx, input(events), |ui| show(ui.ctx(), state));
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
                            node.role(),
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

    fn window() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    /// Where the control a screen reader knows as `label` is.
    fn find(ctx: &egui::Context, state: &mut TesseraApp, label: &str) -> Rect {
        // A window's first frame measures it and takes no clicks.
        draw(ctx, state, Vec::new());
        let nodes = draw(ctx, state, Vec::new());
        nodes
            .iter()
            .find(|(name, role, _)| name == label && *role != egui::accesskit::Role::Label)
            .map(|(_, _, rect)| *rect)
            .unwrap_or_else(|| panic!("no control called {label:?} in {nodes:#?}"))
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

    fn click_at(ctx: &egui::Context, state: &mut TesseraApp, at: egui::Pos2) {
        for pressed in [true, false] {
            draw(ctx, state, press(at, pressed));
        }
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        let at = find(ctx, state, label).center();
        click_at(ctx, state, at);
    }

    fn swatch(state: &TesseraApp, name: &str) -> Swatch {
        state
            .active()
            .document()
            .swatch(name)
            .unwrap_or_else(|| panic!("no swatch {name}"))
            .clone()
    }

    /// A document with the swatch, open in the window.
    fn editing(colour: Color) -> TesseraApp {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::SetSwatch(Swatch::new("Brand", colour)));
        state.swatches_window.chosen = Some("Brand".into());
        state.swatches_window.editing = true;
        state
    }

    fn brand(tint: f32) -> Color {
        Color::Swatch {
            name: "Brand".into(),
            tint,
        }
    }

    const RED: Color = Color::Cmyk {
        c: 0.0,
        m: 0.9,
        y: 0.8,
        k: 0.0,
        a: 1.0,
    };

    /// A rectangle filled with the swatch, and a text frame with a word of
    /// its text set in it.
    fn used(
        state: &mut TesseraApp,
    ) -> (
        tessera_document::ids::FrameId,
        tessera_document::ids::FrameId,
    ) {
        apply(
            state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 50.0,
                height: 50.0,
            }),
        );
        let rectangle = state.active().selection.single().expect("selected");
        apply(
            state,
            Command::SetFill {
                id: rectangle,
                paint: Paint::Solid(brand(0.5)),
            },
        );
        apply(
            state,
            Command::AddTextFrame(tessera_geometry::DocRect {
                x: 0.0,
                y: 100.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let text = state.active().selection.single().expect("selected");
        apply(
            state,
            Command::SetText {
                id: text,
                text: "Plain and coloured".into(),
            },
        );
        let tessera_document::nodes::FrameKind::Text { story, .. } =
            state.active().document().frames[text].kind
        else {
            panic!("a text frame");
        };
        apply(
            state,
            Command::SetCharacterFormat {
                story,
                range: 10..18,
                format: tessera_text::story::CharacterFormat {
                    colour: Some(brand(1.0)),
                    ..Default::default()
                },
            },
        );
        state.active_mut().selection.clear();
        (rectangle, text)
    }

    #[test]
    fn a_cmyk_swatch_is_edited_in_inks_and_stays_cmyk() {
        // The old picker was RGB only, and touching a CMYK swatch with it
        // turned it into RGB: another colour on another set of plates.
        let mut state = editing(RED);
        let ctx = window();
        // The middle of the magenta track is 50%.
        click(&ctx, &mut state, "Magenta");
        let Color::Cmyk { c, m, y, k, .. } = swatch(&state, "Brand").colour else {
            panic!("still CMYK: {:?}", swatch(&state, "Brand").colour);
        };
        assert_eq!([c, y, k], [0.0, 0.8, 0.0], "the other inks are left alone");
        assert!((m - 0.5).abs() < 0.02, "magenta went to the middle: {m}");
    }

    #[test]
    fn changing_the_mode_converts_the_colour_rather_than_reading_its_numbers_anew() {
        let teal = Color::Rgb {
            r: 0.2,
            g: 0.6,
            b: 0.6,
            a: 1.0,
        };
        let mut state = editing(teal.clone());
        let ctx = window();
        let looks = |state: &TesseraApp| swatch(state, "Brand").colour.to_rgb_f32();
        let before = looks(&state);

        click(&ctx, &mut state, "CMYK");
        assert_eq!(Mode::of(&swatch(&state, "Brand").colour), Some(Mode::Cmyk));
        click(&ctx, &mut state, "Lab");
        assert_eq!(Mode::of(&swatch(&state, "Brand").colour), Some(Mode::Lab));
        click(&ctx, &mut state, "RGB");
        let after = looks(&state);
        for (a, b) in before.iter().zip(after) {
            assert!((a - b).abs() < 0.01, "{before:?} came back as {after:?}");
        }
    }

    #[test]
    fn a_swatch_made_spot_is_one_step_to_undo() {
        let mut state = editing(RED);
        let ctx = window();
        click(&ctx, &mut state, "Spot");
        assert!(swatch(&state, "Brand").spot);
        apply(&mut state, Command::Undo);
        assert!(!swatch(&state, "Brand").spot);
    }

    #[test]
    fn dragging_a_slider_follows_live_and_undoes_as_one_step() {
        // A step per frame of the drag would take forty undos to take back
        // one gesture; no step until the button came up would leave the page
        // still while the slider moved.
        let mut state = editing(RED);
        let ctx = window();
        let track = find(&ctx, &mut state, "Cyan");
        let y = track.center().y;
        let mut x = track.left() + 10.0;
        draw(&ctx, &mut state, press(egui::pos2(x, y), true));
        let mut seen = Vec::new();
        for _ in 0..4 {
            x += 30.0;
            draw(
                &ctx,
                &mut state,
                vec![egui::Event::PointerMoved(egui::pos2(x, y))],
            );
            seen.push(channel_values(&swatch(&state, "Brand").colour)[0]);
        }
        draw(&ctx, &mut state, press(egui::pos2(x, y), false));
        draw(&ctx, &mut state, Vec::new());
        assert!(
            seen.windows(2).all(|w| w[1] > w[0]),
            "the document followed the drag: {seen:?}"
        );
        assert!(
            state.swatches_window.dragging.is_none(),
            "and the drag ended"
        );

        apply(&mut state, Command::Undo);
        assert_eq!(swatch(&state, "Brand").colour, RED, "one undo, all of it");
    }

    #[test]
    fn next_visits_each_object_then_each_stretch_of_text() {
        let mut state = editing(RED);
        let (rectangle, text) = used(&mut state);
        let ctx = window();

        click(&ctx, &mut state, "Next use");
        assert_eq!(state.active().selection.single(), Some(rectangle));
        click(&ctx, &mut state, "Next use");
        assert_eq!(state.active().selection.single(), Some(text));
        let selected = state
            .active()
            .editing
            .as_ref()
            .and_then(|(_, buffer)| buffer.selection_range());
        assert_eq!(selected, Some(10..18), "the coloured word, selected");
        click(&ctx, &mut state, "Next use");
        assert_eq!(
            state.active().selection.single(),
            Some(rectangle),
            "round again"
        );
    }

    #[test]
    fn the_uses_are_counted_in_every_place_they_are() {
        let mut state = editing(RED);
        used(&mut state);
        apply(
            &mut state,
            Command::DefineParagraphStyle(tessera_text::story::ParagraphStyle {
                name: "Headline".into(),
                based_on: None,
                format: tessera_text::story::ParagraphFormat {
                    character: tessera_text::story::CharacterFormat {
                        colour: Some(brand(1.0)),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            }),
        );
        let uses = uses_of(&mut state, "Brand");
        assert_eq!(uses.references.frames.len(), 1);
        assert_eq!(uses.text_places(), 1);
        assert_eq!(uses.styles(), 1);
        assert_eq!(uses.places(), 3);
        assert_eq!(
            uses.sentence(),
            "Used by 1 object, 1 place in the text and 1 style."
        );
    }

    #[test]
    fn deleting_a_used_swatch_asks_and_the_uses_keep_their_colour() {
        let mut state = editing(RED);
        let (rectangle, _) = used(&mut state);
        let ctx = window();
        let shown = state.active().document().resolve_colour(&brand(0.5));

        click(&ctx, &mut state, "Delete…");
        assert!(state.swatches_window.deleting.is_some(), "asked, not done");
        assert!(state.active().document().swatch("Brand").is_some());

        click(&ctx, &mut state, "Delete");
        assert!(state.active().document().swatch("Brand").is_none());
        assert_eq!(
            state.active().document().frames[rectangle].fill,
            Paint::Solid(shown),
            "the half tint it was, as a colour of its own"
        );
        assert!(!state.swatches_window.editing, "nothing left to edit");
    }

    #[test]
    fn deleting_a_used_swatch_can_hand_its_uses_to_another() {
        let mut state = editing(RED);
        apply(
            &mut state,
            Command::SetSwatch(Swatch::new("House", Color::BLACK)),
        );
        let (rectangle, _) = used(&mut state);
        let ctx = window();
        click(&ctx, &mut state, "Delete…");
        click(&ctx, &mut state, "Use another swatch instead");
        click(&ctx, &mut state, "Delete");
        assert_eq!(
            state.active().document().frames[rectangle].fill,
            Paint::Solid(Color::Swatch {
                name: "House".into(),
                tint: 0.5
            }),
            "the other swatch, at the tint the use named"
        );
    }

    #[test]
    fn deleting_an_unused_swatch_does_not_ask() {
        let mut state = editing(RED);
        let ctx = window();
        click(&ctx, &mut state, "Delete…");
        assert!(state.swatches_window.deleting.is_none());
        assert!(state.active().document().swatch("Brand").is_none());
    }

    #[test]
    fn a_step_of_the_tint_ramp_becomes_a_tint_swatch_of_it() {
        let mut state = editing(RED);
        let ctx = window();
        click(&ctx, &mut state, "Make a 40% tint swatch");
        assert_eq!(swatch(&state, "Brand 40%").colour, brand(0.4));
        assert_eq!(
            state.swatches_window.chosen.as_deref(),
            Some("Brand 40%"),
            "and the window shows it"
        );
        // A tint of it follows it: the page draws 40% of whatever Brand is.
        apply(
            &mut state,
            Command::EditSwatch {
                old: "Brand".into(),
                swatch: Swatch::new("Brand", Color::BLACK),
            },
        );
        let resolved = state.active().document().resolve_colour(&Color::Swatch {
            name: "Brand 40%".into(),
            tint: 1.0,
        });
        assert_eq!(resolved, Color::BLACK.tinted(0.4));
    }

    #[test]
    fn the_name_is_typed_over_in_place_and_every_use_follows() {
        let mut state = editing(RED);
        let (rectangle, _) = used(&mut state);
        let ctx = window();
        click(&ctx, &mut state, "Swatch name");
        draw(
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
                egui::Event::Text("House red".into()),
            ],
        );
        draw(
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
        assert!(state.active().document().swatch("House red").is_some());
        assert_eq!(state.swatches_window.chosen.as_deref(), Some("House red"));
        assert_eq!(
            state.active().document().frames[rectangle].fill,
            Paint::Solid(Color::Swatch {
                name: "House red".into(),
                tint: 0.5
            })
        );
    }

    #[test]
    fn a_name_that_cannot_be_one_is_refused() {
        let mut state = editing(RED);
        apply(
            &mut state,
            Command::SetSwatch(Swatch::new("Taken", Color::BLACK)),
        );
        assert!(name_problem(&state, Some("Brand"), "Taken").is_some());
        assert!(name_problem(&state, Some("Brand"), "  ").is_some());
        assert!(name_problem(&state, Some("Brand"), "[Black]").is_some());
        assert!(
            name_problem(&state, Some("Brand"), "Brand").is_none(),
            "its own"
        );
        assert!(name_problem(&state, Some("Brand"), "Fresh").is_none());
    }

    #[test]
    fn a_colour_reads_in_its_own_numbers() {
        assert_eq!(numbers(&RED), "C 0  M 90  Y 80  K 0");
        assert_eq!(numbers(&brand(0.4)), "40% of Brand");
        let rgb = Color::Rgb {
            r: 200.0 / 255.0,
            g: 16.0 / 255.0,
            b: 46.0 / 255.0,
            a: 1.0,
        };
        assert_eq!(numbers(&rgb), "R 200  G 16  B 46");
        assert_eq!(hex(&rgb).as_deref(), Some("#C8102E"));
        assert_eq!(
            parse_hex("c8102e"),
            Some([200.0, 16.0, 46.0].map(|v| v / 255.0))
        );
        assert_eq!(parse_hex("#C8102"), None);
        assert_eq!(parse_hex("#G8102E"), None);
    }

    #[test]
    fn a_spot_is_edited_through_its_stand_in_and_keeps_its_name() {
        let spot = Color::Spot {
            name: "PANTONE 185 C".into(),
            tint: 1.0,
            fallback: Box::new(RED),
        };
        let edited = with_channel(&spot, 3, 20.0);
        let Color::Spot { name, fallback, .. } = &edited else {
            panic!("still a spot");
        };
        assert_eq!(name, "PANTONE 185 C");
        assert!(matches!(**fallback, Color::Cmyk { k, .. } if (k - 0.2).abs() < 1e-6));
        let converted = convert(&spot, Mode::Rgb, Press::default());
        assert!(
            matches!(&converted, Color::Spot { fallback, .. } if Mode::of(fallback) == Some(Mode::Rgb))
        );
    }

    #[test]
    fn rgb_black_converts_to_the_black_plate_alone() {
        // As the PDF writer makes it: a rich black for a rule of type would
        // fringe in four colours the moment the plates slipped.
        let converted = convert(&Color::BLACK, Mode::Cmyk, Press::default());
        assert_eq!(converted, BLACK_INK);
    }

    #[test]
    fn the_counts_follow_the_document_as_it_changes() {
        // Found once per revision, not once per frame — and so found again
        // the moment a use is added, or the panel's number would lie.
        let mut state = editing(RED);
        assert_eq!(use_counts(&mut state), [("Brand".to_string(), 0)]);
        used(&mut state);
        assert_eq!(use_counts(&mut state), [("Brand".to_string(), 2)]);
        apply(&mut state, Command::Undo);
        assert_eq!(use_counts(&mut state), [("Brand".to_string(), 1)]);
    }

    /// The press the checkout carries, if it does: its proof and its
    /// separation, as the window takes them from the soft proof.
    fn a_press() -> Option<(Proof, Conversion)> {
        let press = tessera_color::profiles::bundled()
            .into_iter()
            .find(|b| b.space == "CMYK")?;
        let profile =
            tessera_color::managed::OutputProfile::from_bytes(std::fs::read(press.path).ok()?)
                .ok()?;
        let rendering = tessera_color::managed::Rendering::default();
        Some((
            profile.proof(rendering).ok()?,
            profile.ink_for_screen_colour(rendering).ok()?,
        ))
    }

    #[test]
    fn with_a_press_a_screen_colour_becomes_the_inks_that_press_would_use() {
        let Some((proof, ink)) = a_press() else {
            return;
        };
        let press = Press {
            proof: Some(&proof),
            ink: Some(&ink),
        };
        let grey = Color::Rgb {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let Color::Cmyk { c, m, y, k, .. } = convert(&grey, Mode::Cmyk, press) else {
            panic!("inks");
        };
        // A press builds grey from all four; the formula from K alone.
        assert!(c > 0.1 && m > 0.1 && y > 0.1 && k > 0.0, "{c} {m} {y} {k}");
        assert!([c, m, y, k].iter().all(|v| *v <= 1.0));
        // And pure black is the black plate alone, where the press would
        // answer a rich black.
        assert_eq!(convert(&Color::BLACK, Mode::Cmyk, press), BLACK_INK);
        // Inks back to the screen go through the press too: solid cyan is
        // the press's cyan, not the formula's pure 0/255/255.
        let Color::Rgb { r, g, b, .. } = convert(
            &Color::Cmyk {
                c: 1.0,
                m: 0.0,
                y: 0.0,
                k: 0.0,
                a: 1.0,
            },
            Mode::Rgb,
            press,
        ) else {
            panic!("rgb");
        };
        assert!(r < 0.4 && g < 0.9 && b > 0.7, "{r} {g} {b}");
    }
}
