//! The console: where a person talks to a model about the page.
//!
//! A docked panel — a transcript above, one line to type in below — and
//! nothing else, because the model's work shows on the canvas, not here.
//! Each tool the model runs is one line in the transcript ("add_text_frame
//! → frame 3"), and one undo entry, so a bad turn is Ctrl+Z like a bad
//! click.
//!
//! This panel knows no model. It holds the words and hands out the
//! prompts: whoever drives the model — the bridge, in the binary — takes
//! them from [`Console::outbox`], does the round trips on a thread of its
//! own, and puts what happened back with [`Console::heard`]. Keeping the
//! network out of this crate is what keeps the panel testable and what
//! keeps this crate free of an HTTP client, as the update check is.

use std::path::Path;

use egui::{Sense, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};

use super::style_ui;
use crate::app::TesseraApp;
use crate::icons::Icon;
use crate::theme::Theme;

/// Where the transcript is kept between runs, beside the preferences.
pub const TRANSCRIPT_FILE: &str = "console.json";
/// How many lines are kept: enough to read back what was done this week,
/// not a log.
pub const TRANSCRIPT_KEPT: usize = 400;

/// One line of the transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Line {
    /// What the person typed.
    You(String),
    /// What the model said.
    Model(String),
    /// A tool the model ran, and how it went.
    Tool {
        name: String,
        summary: String,
        is_error: bool,
    },
    /// Something that went wrong between here and the model.
    Error(String),
    /// A word from the console itself.
    Note(String),
}

/// One turn: a prompt and everything the model did in answer, as undo sees
/// it — so the whole of it can be undone at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Turn {
    /// The document the turn began and ended in; a turn that moved between
    /// documents is not undone as one.
    pub document: crate::app::DocumentKey,
    /// The document's edits recorded and its undo depth when the turn began.
    pub recorded_before: u64,
    /// The same when it ended; `None` while it runs.
    pub after: Option<(crate::app::DocumentKey, u64, usize)>,
}

#[derive(Debug, Default)]
pub struct Console {
    pub open: bool,
    pub input: String,
    pub transcript: Vec<Line>,
    /// This session's turns, the latest last. Not kept between runs: the
    /// undo history they point into is not either.
    pub turns: Vec<Turn>,
    /// Which earlier prompt the input is showing, counted back from the
    /// latest, while Up and Down are walking them.
    recalling: Option<usize>,
    /// Prompts typed and not yet taken by the driver.
    pub outbox: Vec<String>,
    /// Whether a turn is under way: the input is disabled, Stop is shown.
    pub busy: bool,
    /// The person pressed Stop; the driver reads and clears it.
    pub stop_requested: bool,
    /// Set when a new line arrived, so the transcript scrolls to it.
    scroll_to_end: bool,
}

impl Console {
    /// Something happened; show it.
    pub fn heard(&mut self, line: Line) {
        self.transcript.push(line);
        self.scroll_to_end = true;
    }

    /// The last line grew — a model's words arriving — so the transcript
    /// keeps its end in view.
    pub fn touched(&mut self) {
        self.scroll_to_end = true;
    }

    /// A prompt the person sent, for the driver to take.
    pub fn take_prompt(&mut self) -> Option<String> {
        if self.outbox.is_empty() {
            None
        } else {
            Some(self.outbox.remove(0))
        }
    }

    /// Start over: nothing said, nothing remembered by the model either —
    /// the driver drops its session when it sees the transcript empty.
    pub fn clear(&mut self) {
        self.transcript.clear();
        self.outbox.clear();
        self.turns.clear();
        self.recalling = None;
    }

    /// A turn is starting, in `document`, with its history as it stands:
    /// for the driver to call.
    pub fn turn_began(&mut self, document: crate::app::DocumentKey, recorded: u64) {
        self.turns.push(Turn {
            document,
            recorded_before: recorded,
            after: None,
        });
    }

    /// The turn under way has ended, in `document`, with its history so.
    pub fn turn_ended(&mut self, document: crate::app::DocumentKey, recorded: u64, depth: usize) {
        if let Some(turn) = self.turns.last_mut() {
            turn.after = Some((document, recorded, depth));
        }
    }

    /// How many changes the last turn made, when undoing them all is still
    /// exactly undoing the turn: it ended where it began, the document is the
    /// one in front, nothing has been done or undone since, and every entry
    /// it made is still in the history.
    pub fn undoable_turn(
        &self,
        active: crate::app::DocumentKey,
        recorded: u64,
        depth: usize,
    ) -> Option<usize> {
        let turn = self.turns.last()?;
        let (document, recorded_after, depth_after) = turn.after?;
        let made = recorded_after.checked_sub(turn.recorded_before)? as usize;
        (made > 0
            && document == turn.document
            && document == active
            && recorded == recorded_after
            && depth == depth_after
            && depth_after >= made)
            .then_some(made)
    }

    /// The prompts sent, the latest first.
    fn prompts(&self) -> impl Iterator<Item = &str> {
        self.transcript.iter().rev().filter_map(|line| match line {
            Line::You(text) => Some(text.as_str()),
            _ => None,
        })
    }

    /// Show the prompt before the one shown — or the latest — in the input,
    /// as a terminal's Up does. Whether there was one.
    fn recall_earlier(&mut self) -> bool {
        let next = self.recalling.map_or(0, |at| at + 1);
        let Some(prompt) = self.prompts().nth(next).map(str::to_owned) else {
            return false;
        };
        self.input = prompt;
        self.recalling = Some(next);
        true
    }

    /// Show the prompt after the one shown, or an empty input past the
    /// latest.
    fn recall_later(&mut self) -> bool {
        let Some(at) = self.recalling else {
            return false;
        };
        if at == 0 {
            self.input.clear();
            self.recalling = None;
            return true;
        }
        let prompt = self.prompts().nth(at - 1).map(str::to_owned);
        if let Some(prompt) = prompt {
            self.input = prompt;
            self.recalling = Some(at - 1);
        }
        true
    }

    /// Write the transcript out — its last [`TRANSCRIPT_KEPT`] lines — for
    /// the next run to read back. Quietly: a transcript that could not be
    /// kept is not worth an error over the work.
    pub fn save_to(&self, path: &Path) {
        if self.transcript.is_empty() {
            let _ = std::fs::remove_file(path);
            return;
        }
        let from = self.transcript.len().saturating_sub(TRANSCRIPT_KEPT);
        let kept = &self.transcript[from..];
        if let Ok(json) = serde_json::to_vec(kept) {
            let _ = tessera_io::atomic::write_atomic(path, &json);
        }
    }

    /// Read back what the last run saved, with a note that the model does
    /// not remember any of it — its session starts afresh, and a person
    /// who refers to "the frame you made" should know why the model asks
    /// which. Nothing to read back is nothing said.
    pub fn restore_from(&mut self, path: &Path) {
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let Ok(lines) = serde_json::from_slice::<Vec<Line>>(&bytes) else {
            return;
        };
        if lines.is_empty() {
            return;
        }
        self.transcript = lines;
        self.heard(Line::Note(
            "Restored from the last session. The model starts afresh and remembers none of it."
                .into(),
        ));
    }

    /// Send what is in the input, if anything.
    fn send(&mut self) {
        let text = self.input.trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.input.clear();
        self.recalling = None;
        self.heard(Line::You(text.clone()));
        self.outbox.push(text);
    }
}

/// A tool's name as a line says it: `add_text_frame` is "Add text frame".
pub fn humanise(tool: &str) -> String {
    let mut words = tool.replace('_', " ");
    if let Some(first) = words.get(0..1) {
        let upper = first.to_uppercase();
        words.replace_range(0..1, &upper);
    }
    words
}

/// What a model writes, laid out: its `**strong**` words in the heavier
/// face and its `code` in the monospaced one, and a line beginning "- " or
/// "* " as a bulleted one — the marks themselves read as noise, and every
/// model writes them.
pub fn rich(text: &str, size: f32, colour: egui::Color32, width: f32) -> egui::text::LayoutJob {
    use egui::{FontFamily, FontId, TextFormat};
    let plain = TextFormat::simple(FontId::proportional(size), colour);
    let strong = TextFormat::simple(
        FontId::new(
            size,
            FontFamily::Name(crate::ui_fonts::HEADING_FAMILY.into()),
        ),
        colour,
    );
    let code = TextFormat {
        font_id: FontId::monospace(size - 1.0),
        color: colour,
        background: Theme::hover_bg(),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    for (n, line) in text.lines().enumerate() {
        if n > 0 {
            job.append("\n", 0.0, plain.clone());
        }
        let trimmed = line.trim_start();
        let line = match trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            Some(rest) => {
                job.append("\u{2022} ", 0.0, plain.clone());
                rest
            }
            None => line,
        };
        let (mut bold, mut mono) = (false, false);
        let mut run = String::new();
        let mut chars = line.chars().peekable();
        let flush = |job: &mut egui::text::LayoutJob, run: &mut String, bold: bool, mono: bool| {
            if run.is_empty() {
                return;
            }
            let format = if mono {
                code.clone()
            } else if bold {
                strong.clone()
            } else {
                plain.clone()
            };
            job.append(run, 0.0, format);
            run.clear();
        };
        while let Some(c) = chars.next() {
            if c == '`' {
                flush(&mut job, &mut run, bold, mono);
                mono = !mono;
            } else if c == '*' && !mono && chars.peek() == Some(&'*') {
                chars.next();
                flush(&mut job, &mut run, bold, mono);
                bold = !bold;
            } else {
                run.push(c);
            }
        }
        flush(&mut job, &mut run, bold, mono);
    }
    job.wrap.max_width = width;
    job
}

/// Prompts offered before anything is said: what the console is for, one
/// click from being asked.
pub const SUGGESTIONS: [&str; 4] = [
    "Describe what is on this page",
    "Add a headline across the top of this page",
    "Set the selected text in two columns",
    "Check this page for anything that will not print well",
];

/// What the input area left the transcript last frame, before it has been
/// measured.
const INPUT_ROOM: f32 = 120.0;

/// What a click in the panel asked for, done once it is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Act {
    Send,
    Ask(String),
    Stop,
    NewChat,
    UndoTurn(usize),
    Settings,
    Copy(String),
}

/// Whether a model is set up to be talked to.
fn configured(state: &TesseraApp) -> bool {
    !state.prefs.assistant.model.trim().is_empty()
        && (state.prefs.assistant.provider != "anthropic"
            || !state.prefs.assistant.api_key.trim().is_empty())
}

/// The panel, in the rail.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    let configured = configured(state);
    let mut act = header(ui, state, configured);

    if !configured {
        super::panel_ui::empty(
            ui,
            "Set up your assistant",
            "Choose a provider, a model and a key in Preferences \u{203a} General \u{203a} \
             Assistant. The console talks to that model, and it works on the page you have open.",
        );
        if super::panel_ui::action(ui, Icon::Properties, "Open preferences").clicked() {
            act = Some(Act::Settings);
        }
    } else if state.console.transcript.is_empty() && !state.console.busy {
        if let Some(asked) = suggestions(ui) {
            act = Some(asked);
        }
    } else if let Some(asked) = transcript(ui, state) {
        act = Some(asked);
    }

    if configured && let Some(asked) = input(ui, state) {
        act = Some(asked);
    }

    if let Some(act) = act {
        run(ui.ctx(), state, act);
    }
}

/// The model talked to, whether it is working, and what starts over or stops.
fn header(ui: &mut Ui, state: &TesseraApp, configured: bool) -> Option<Act> {
    let mut act = None;
    let busy = state.console.busy;
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        let tint = if !configured {
            Theme::text_muted()
        } else if busy {
            Theme::accent()
        } else {
            Theme::ok()
        };
        ui.painter().circle_filled(dot.center(), 4.0, tint);
        let (model, said) = if configured {
            (
                state.prefs.assistant.model.trim().to_owned(),
                if busy {
                    format!("{} \u{00b7} working", state.prefs.assistant.provider)
                } else {
                    state.prefs.assistant.provider.clone()
                },
            )
        } else {
            ("No model set".to_owned(), "Not set up".to_owned())
        };
        let room = (ui.available_width() - 96.0).max(40.0);
        ui.vertical(|ui| {
            ui.set_max_width(room);
            ui.add(
                egui::Label::new(egui::RichText::new(model).color(Theme::text_primary()))
                    .truncate(),
            );
            ui.label(
                egui::RichText::new(said)
                    .size(Theme::TYPE_SM)
                    .color(Theme::text_muted()),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if busy {
                if super::panel_ui::action(ui, Icon::Close, "Stop")
                    .on_hover_text("Stop the model after the step it is on")
                    .clicked()
                {
                    act = Some(Act::Stop);
                }
            } else if !state.console.transcript.is_empty()
                && super::panel_ui::action(ui, Icon::Plus, "New chat")
                    .on_hover_text(
                        "Start over: the transcript is cleared, and the model forgets it too",
                    )
                    .clicked()
            {
                act = Some(Act::NewChat);
            }
        });
    });
    ui.add_space(4.0);
    act
}

/// Before anything is said: what the console is for, and some things to ask.
fn suggestions(ui: &mut Ui) -> Option<Act> {
    let mut act = None;
    super::panel_ui::empty(
        ui,
        "What would you like to make?",
        "Ask for something on the page in your own words. Each change the model makes is \
         one step to undo, and the whole of a turn can be undone at once.",
    );
    ui.add_space(4.0);
    style_ui::overline(ui, "Try");
    for prompt in SUGGESTIONS {
        if style_ui::page_link(ui, Icon::SquareTerminal, prompt, ui.available_width())
            .on_hover_text("Ask it")
            .clicked()
        {
            act = Some(Act::Ask(prompt.to_owned()));
        }
    }
    act
}

/// The conversation: the prompts in bubbles at the right, the model's
/// replies at the left, and between them what it did, one step to a line.
fn transcript(ui: &mut Ui, state: &mut TesseraApp) -> Option<Act> {
    let mut act = None;
    // As tall as the panel leaves above the input, which is measured: the
    // rail hands a panel endless height, and a transcript sized to its
    // contents pushed the input below the window.
    let input_id = egui::Id::new("console-input-height");
    let below = ui
        .data(|d| d.get_temp::<f32>(input_id))
        .unwrap_or(INPUT_ROOM);
    let room = (ui.clip_rect().bottom() - ui.cursor().top() - below).max(120.0);
    let scroll_to_end = std::mem::take(&mut state.console.scroll_to_end);
    let undoable = {
        let open = state.active();
        state.console.undoable_turn(
            state.active,
            open.history.recorded(),
            open.history.undo_depth(),
        )
    };
    egui::ScrollArea::vertical()
        .id_salt("console-transcript")
        .max_height(room)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            let lines = &state.console.transcript;
            let mut at = 0;
            while at < lines.len() {
                match &lines[at] {
                    Line::You(text) => {
                        if let Some(copied) = prompt_bubble(ui, text) {
                            act = Some(copied);
                        }
                    }
                    Line::Model(text) => {
                        if let Some(copied) = reply(ui, text) {
                            act = Some(copied);
                        }
                    }
                    Line::Tool { .. } => {
                        // A run of steps, together: one turn's work.
                        let run = lines[at..]
                            .iter()
                            .take_while(|l| matches!(l, Line::Tool { .. }))
                            .count();
                        steps(ui, &lines[at..at + run]);
                        at += run;
                        continue;
                    }
                    Line::Error(text) => problem(ui, text),
                    Line::Note(text) => note(ui, text),
                }
                at += 1;
            }
            if state.console.busy {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(Theme::accent()));
                    ui.label(
                        egui::RichText::new("Working\u{2026}")
                            .size(Theme::TYPE_SM)
                            .color(Theme::text_muted()),
                    );
                });
            } else if let Some(made) = undoable {
                let label = if made == 1 {
                    "Undo this turn".to_owned()
                } else {
                    format!("Undo this turn ({made} changes)")
                };
                if super::panel_ui::action(ui, Icon::RotateCcw, &label)
                    .on_hover_text("Take back everything the model just did, in one step")
                    .clicked()
                {
                    act = Some(Act::UndoTurn(made));
                }
            }
            if scroll_to_end {
                ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
            }
        });
    act
}

/// What the person asked, in a bubble at the right.
fn prompt_bubble(ui: &mut Ui, text: &str) -> Option<Act> {
    let width = ui.available_width();
    let mut act = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        let inner = egui::Frame::new()
            .fill(Theme::accent_soft())
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_max_width(width * 0.85 - 20.0);
                // Read from the left inside the bubble, though the bubble
                // itself sits at the right.
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).color(Theme::text_primary()))
                            .wrap()
                            .selectable(true),
                    )
                })
                .inner
            });
        inner.response.context_menu(|ui| {
            if ui.button("Copy").clicked() {
                act = Some(Act::Copy(text.to_owned()));
                ui.close();
            }
        });
        crate::icons::speak_as(inner.inner, &format!("You: {text}"));
    });
    act
}

/// What the model said, as it wrote it, marks and all turned into type.
fn reply(ui: &mut Ui, text: &str) -> Option<Act> {
    let mut act = None;
    let job = rich(
        text,
        egui::TextStyle::Body.resolve(ui.style()).size,
        Theme::text_primary(),
        ui.available_width(),
    );
    let response = ui.add(egui::Label::new(job).wrap().selectable(true));
    response.context_menu(|ui| {
        if ui.button("Copy").clicked() {
            act = Some(Act::Copy(text.to_owned()));
            ui.close();
        }
    });
    act
}

/// A run of the model's steps: each tool it ran, by name, with what came
/// of it — a tick or a cross, not only a colour.
fn steps(ui: &mut Ui, lines: &[Line]) {
    egui::Frame::new()
        .fill(style_ui::well_fill())
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            for line in lines {
                let Line::Tool {
                    name,
                    summary,
                    is_error,
                } = line
                else {
                    continue;
                };
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    let (icon, tint) = if *is_error {
                        (Icon::ErrorMark, Theme::error())
                    } else {
                        (Icon::Preflight, Theme::text_muted())
                    };
                    crate::icons::paint(ui.painter(), mark, icon, tint);
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &humanise(name),
                        0.0,
                        egui::TextFormat::simple(
                            egui::FontId::proportional(Theme::TYPE_SM),
                            Theme::text_primary(),
                        ),
                    );
                    if !summary.is_empty() && summary != "ok" {
                        job.append(
                            &format!(" \u{00b7} {summary}"),
                            0.0,
                            egui::TextFormat::simple(
                                egui::FontId::proportional(Theme::TYPE_SM),
                                if *is_error {
                                    Theme::error()
                                } else {
                                    Theme::text_muted()
                                },
                            ),
                        );
                    }
                    job.wrap.max_width = ui.available_width();
                    ui.add(egui::Label::new(job).wrap())
                        .on_hover_text(format!("{name}: {summary}"));
                });
            }
        });
}

/// Something that went wrong between here and the model.
fn problem(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(Theme::error().gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, Theme::error().gamma_multiply(0.4)))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (mark, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                crate::icons::paint(ui.painter(), mark, Icon::ErrorMark, Theme::error());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .size(Theme::TYPE_SM)
                            .color(Theme::text_primary()),
                    )
                    .wrap(),
                );
            });
        });
}

/// A word from the console itself, between rules, out of the way.
fn note(ui: &mut Ui, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .size(Theme::TYPE_SM)
                    .color(Theme::text_muted()),
            )
            .wrap(),
        );
    });
}

/// Where the prompt is written: several lines, Enter to send, Shift+Enter
/// for a new line, Up for the prompts sent before.
fn input(ui: &mut Ui, state: &mut TesseraApp) -> Option<Act> {
    let mut act = None;
    let top = ui.cursor().top();
    let busy = state.console.busy;
    let id = egui::Id::new("console-input");
    let focused = ui.memory(|m| m.has_focus(id));
    if focused && !busy {
        // Taken before the text box sees them: Enter sends, where a text
        // box would start a line, and Up and Down walk the prompts while the
        // box is empty or showing one.
        let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if enter {
            act = Some(Act::Send);
        }
        let walking = state.console.input.is_empty() || state.console.recalling.is_some();
        if walking {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                state.console.recall_earlier();
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                state.console.recall_later();
            }
        }
    }
    ui.add_space(4.0);
    let response = ui.add_enabled(
        !busy,
        egui::TextEdit::multiline(&mut state.console.input)
            .id(id)
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .hint_text("Ask for something on the page\u{2026}"),
    );
    crate::icons::speak_as(response.clone(), "Prompt");
    if response.changed() {
        // Typed over: no longer walking the old prompts.
        state.console.recalling = None;
    }
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Enter to send \u{00b7} Shift+Enter for a new line")
                .size(Theme::TYPE_SM - 1.0)
                .color(Theme::text_muted()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ready = !busy && !state.console.input.trim().is_empty();
            if super::panel_ui::action_when(ui, ready, Icon::ChevronRight, "Send")
                .on_hover_text("Send the prompt (Enter)")
                .clicked()
            {
                act = Some(Act::Send);
            }
        });
    });
    if matches!(act, Some(Act::Send)) {
        response.request_focus();
    }
    let height = ui.cursor().top() - top + 8.0;
    let input_id = egui::Id::new("console-input-height");
    if ui.data(|d| d.get_temp::<f32>(input_id)) != Some(height) {
        ui.data_mut(|d| d.insert_temp(input_id, height));
        ui.ctx().request_repaint();
    }
    act
}

fn run(ctx: &egui::Context, state: &mut TesseraApp, act: Act) {
    match act {
        Act::Send => state.console.send(),
        Act::Ask(prompt) => {
            state.console.input = prompt;
            state.console.send();
        }
        Act::Stop => state.console.stop_requested = true,
        Act::NewChat => state.console.clear(),
        Act::UndoTurn(made) => {
            for _ in 0..made {
                crate::command::apply(state, crate::command::Command::Undo);
            }
            state.console.heard(Line::Note(format!(
                "Undid the last turn: {}.",
                if made == 1 {
                    "1 change".to_owned()
                } else {
                    format!("{made} changes")
                }
            )));
        }
        Act::Settings => {
            state.settings.open = true;
            state.settings.page = super::settings::Page::General;
        }
        Act::Copy(text) => ctx.copy_text(text),
    }
}

/// Where the model is told what the person is looking at, ahead of what
/// they said: the selection, the page in view, the document's frames. Not
/// a system prompt, which is set once — this is true now.
pub fn context_line(state: &TesseraApp) -> String {
    let open = state.active();
    let doc = open.document();
    let selected: Vec<String> = open
        .selection
        .iter()
        .map(|id| slotmap::Key::data(&id).as_ffi().to_string())
        .collect();
    let page = doc
        .spread_ids()
        .nth(open.current_spread)
        .and_then(|s| doc.spreads.get(s))
        .and_then(|s| s.pages.first().copied())
        .and_then(|p| doc.page_ids().position(|q| q == p));
    format!(
        "[Context: {} page(s), {} frame(s); page {} in view; selection: {}. Frame numbers are those describe_document reports.]",
        doc.page_ids().count(),
        doc.frames.len(),
        page.map_or("?".to_owned(), |p| p.to_string()),
        if selected.is_empty() {
            "none".to_owned()
        } else {
            selected.join(", ")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prompt_goes_to_the_transcript_and_the_outbox_once() {
        let mut console = Console {
            input: "  make a headline  ".into(),
            ..Console::default()
        };
        console.send();
        assert_eq!(
            console.transcript,
            vec![Line::You("make a headline".into())]
        );
        assert_eq!(console.take_prompt(), Some("make a headline".into()));
        assert_eq!(console.take_prompt(), None);
        assert!(console.input.is_empty());
        // Nothing from nothing.
        console.send();
        assert_eq!(console.transcript.len(), 1);
    }

    #[test]
    fn a_tool_is_named_as_a_line_says_it() {
        assert_eq!(humanise("add_text_frame"), "Add text frame");
        assert_eq!(humanise("render_page"), "Render page");
        assert_eq!(humanise(""), "");
    }

    /// The pieces a laid-out reply was made of: text and whether it is set
    /// heavy or monospaced.
    fn pieces(text: &str) -> Vec<(String, &'static str)> {
        let job = rich(text, 12.0, egui::Color32::WHITE, 200.0);
        job.sections
            .iter()
            .map(|s| {
                let kind = match &s.format.font_id.family {
                    egui::FontFamily::Monospace => "code",
                    egui::FontFamily::Name(_) => "strong",
                    _ => "plain",
                };
                (
                    job.text[s.byte_range.start.0..s.byte_range.end.0].to_string(),
                    kind,
                )
            })
            .collect()
    }

    #[test]
    fn a_reply_s_marks_become_type_and_are_not_shown() {
        assert_eq!(
            pieces("Set in **Minion Bold** with `Caption` style."),
            vec![
                ("Set in ".into(), "plain"),
                ("Minion Bold".into(), "strong"),
                (" with ".into(), "plain"),
                ("Caption".into(), "code"),
                (" style.".into(), "plain"),
            ]
        );
        assert_eq!(
            pieces("- one\n* two"),
            vec![("\u{2022} one\n\u{2022} two".into(), "plain")],
            "a list's marks become bullets"
        );
        assert_eq!(
            pieces("`a ** b`"),
            vec![("a ** b".into(), "code")],
            "nothing is read inside code"
        );
        assert_eq!(
            pieces("2 * 3"),
            vec![("2 * 3".into(), "plain")],
            "one star is a star"
        );
    }

    #[test]
    fn up_and_down_walk_the_prompts_sent() {
        let mut console = Console::default();
        for prompt in ["first", "second", "third"] {
            console.input = prompt.into();
            console.send();
        }
        assert!(console.recall_earlier());
        assert_eq!(console.input, "third");
        assert!(console.recall_earlier());
        assert_eq!(console.input, "second");
        assert!(console.recall_earlier());
        assert!(!console.recall_earlier(), "nothing before the first");
        assert_eq!(console.input, "first");
        assert!(console.recall_later());
        assert_eq!(console.input, "second");
        console.recall_later();
        console.recall_later();
        assert_eq!(console.input, "", "past the latest, an empty input again");
        assert!(!console.recall_later());
    }

    #[test]
    fn a_turn_is_undone_as_one_only_while_nothing_has_happened_since() {
        let key = TesseraApp::headless().active;
        let mut console = Console::default();
        console.turn_began(key, 10);
        assert_eq!(console.undoable_turn(key, 10, 10), None, "still running");
        console.turn_ended(key, 14, 14);
        assert_eq!(console.undoable_turn(key, 14, 14), Some(4));
        assert_eq!(console.undoable_turn(key, 15, 15), None, "an edit since");
        assert_eq!(console.undoable_turn(key, 14, 13), None, "an undo since");
        assert_eq!(console.undoable_turn(key, 14, 3), None, "trimmed away");
        assert_eq!(
            console.undoable_turn(key, 15, 14),
            None,
            "an undo and an edit since: the depth is back where it was, the edit is not"
        );
        let mut big = Console::default();
        big.turn_began(key, 10);
        big.turn_ended(key, 110, 90);
        assert_eq!(
            big.undoable_turn(key, 110, 90),
            None,
            "a turn larger than the history holds cannot be undone whole"
        );
        console.turn_began(key, 14);
        console.turn_ended(key, 14, 14);
        assert_eq!(
            console.undoable_turn(key, 14, 14),
            None,
            "a turn that changed nothing"
        );
        console.clear();
        assert!(console.turns.is_empty());
    }

    #[test]
    fn the_context_line_says_what_is_in_view() {
        let state = TesseraApp::headless();
        let line = context_line(&state);
        assert!(line.contains("1 page(s)"), "{line}");
        assert!(line.contains("selection: none"), "{line}");
    }

    #[test]
    fn the_transcript_comes_back_after_a_restart_with_a_note_and_no_more_than_kept() {
        let path =
            std::env::temp_dir().join(format!("tessera-console-{}.json", std::process::id()));
        let mut console = Console::default();
        for n in 0..(TRANSCRIPT_KEPT + 50) {
            console.heard(Line::You(format!("prompt {n}")));
        }
        console.heard(Line::Tool {
            name: "add_text_frame".into(),
            summary: "frame 3".into(),
            is_error: false,
        });
        console.save_to(&path);

        let mut back = Console::default();
        back.restore_from(&path);
        assert_eq!(
            back.transcript.len(),
            TRANSCRIPT_KEPT + 1,
            "the last lines, plus the note"
        );
        assert!(matches!(back.transcript.last(), Some(Line::Note(n)) if n.contains("afresh")));
        assert_eq!(
            back.transcript[TRANSCRIPT_KEPT - 1],
            Line::Tool {
                name: "add_text_frame".into(),
                summary: "frame 3".into(),
                is_error: false,
            },
            "the tool line survived with its shape"
        );
        assert!(
            back.outbox.is_empty() && !back.busy,
            "nothing is pending after a restore"
        );

        // An empty transcript takes the file away rather than leaving a
        // stale one to restore next time.
        Console::default().save_to(&path);
        assert!(!path.exists());
        let mut nothing = Console::default();
        nothing.restore_from(&path);
        assert!(
            nothing.transcript.is_empty(),
            "nothing to read back is nothing said"
        );
    }

    #[test]
    fn the_key_stays_in_the_file_until_a_keychain_has_taken_it() {
        // No test touches a real keychain: `held` starts false, so the
        // preferences file carries the key as it always has.
        let prefs = crate::prefs::Preferences {
            assistant: crate::prefs::Assistant {
                provider: "anthropic".into(),
                api_key: "sk-test".into(),
                model: "claude".into(),
                base_url: String::new(),
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(
            json.contains("sk-test"),
            !crate::keychain::held(&String::new()),
            "written to the file exactly when no keychain holds it"
        );
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

    fn a_panel() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    fn find(ctx: &egui::Context, state: &mut TesseraApp, label: &str) -> Option<egui::Rect> {
        panel(ctx, state, Vec::new());
        panel(ctx, state, Vec::new())
            .into_iter()
            .find(|(name, _)| name == label)
            .map(|(_, rect)| rect)
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        let at = find(ctx, state, label)
            .unwrap_or_else(|| panic!("no {label:?}"))
            .center();
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

    fn key(ctx: &egui::Context, state: &mut TesseraApp, key: egui::Key) {
        for pressed in [true, false] {
            panel(
                ctx,
                state,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                }],
            );
        }
    }

    fn set_up() -> TesseraApp {
        let mut state = TesseraApp::headless();
        state.prefs.assistant.provider = "openai".into();
        state.prefs.assistant.model = "test".into();
        state
    }

    #[test]
    fn without_a_model_it_says_where_to_set_one() {
        let mut state = TesseraApp::headless();
        let ctx = a_panel();
        click(&ctx, &mut state, "Open preferences");
        assert!(state.settings.open);
        assert!(
            find(&ctx, &mut state, "Prompt").is_none(),
            "nothing to type into"
        );
    }

    #[test]
    fn a_suggestion_clicked_is_asked() {
        let mut state = set_up();
        let ctx = a_panel();
        click(&ctx, &mut state, SUGGESTIONS[0]);
        assert_eq!(state.console.take_prompt().as_deref(), Some(SUGGESTIONS[0]));
        assert_eq!(
            state.console.transcript,
            vec![Line::You(SUGGESTIONS[0].into())]
        );
    }

    #[test]
    fn enter_sends_and_up_brings_the_prompt_back() {
        let mut state = set_up();
        let ctx = a_panel();
        click(&ctx, &mut state, "Prompt");
        state.console.input = "make a headline".into();
        key(&ctx, &mut state, egui::Key::Enter);
        assert_eq!(
            state.console.take_prompt().as_deref(),
            Some("make a headline")
        );
        assert!(state.console.input.is_empty());
        key(&ctx, &mut state, egui::Key::ArrowUp);
        assert_eq!(state.console.input, "make a headline");
    }

    #[test]
    fn a_turn_s_changes_are_undone_with_one_click() {
        let mut state = set_up();
        let ctx = a_panel();
        state.console.heard(Line::You("two boxes".into()));
        let recorded = state.active().history.recorded();
        state.console.turn_began(state.active, recorded);
        for x in [0.0, 50.0] {
            crate::command::apply(
                &mut state,
                crate::command::Command::AddRectangle(tessera_geometry::DocRect {
                    x,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                }),
            );
        }
        let history = &state.active().history;
        let (recorded, depth) = (history.recorded(), history.undo_depth());
        state.console.turn_ended(state.active, recorded, depth);
        state.console.heard(Line::Model("Two boxes.".into()));
        assert_eq!(state.active().document().frames.len(), 2);

        click(&ctx, &mut state, "Undo this turn (2 changes)");
        assert_eq!(state.active().document().frames.len(), 0, "both, at once");
        assert!(
            find(&ctx, &mut state, "Undo this turn (2 changes)").is_none(),
            "and not offered twice"
        );
    }

    #[test]
    fn stop_and_new_chat_do_what_they_say() {
        let mut state = set_up();
        let ctx = a_panel();
        state.console.heard(Line::You("hello".into()));
        state.console.busy = true;
        click(&ctx, &mut state, "Stop");
        assert!(state.console.stop_requested);
        state.console.busy = false;
        click(&ctx, &mut state, "New chat");
        assert!(state.console.transcript.is_empty());
    }
}
