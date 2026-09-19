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

use egui::Ui;
use serde::{Deserialize, Serialize};

use crate::app::TesseraApp;
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

#[derive(Debug, Default)]
pub struct Console {
    pub open: bool,
    pub input: String,
    pub transcript: Vec<Line>,
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
        self.heard(Line::You(text.clone()));
        self.outbox.push(text);
    }
}

/// How tall the transcript grows before it scrolls, in points.
const TRANSCRIPT_HEIGHT: f32 = 320.0;

/// The panel, in the rail.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    let configured = !state.prefs.assistant.model.trim().is_empty()
        && (state.prefs.assistant.provider != "anthropic"
            || !state.prefs.assistant.api_key.trim().is_empty());

    ui.horizontal(|ui| {
        ui.colored_label(
            Theme::text_muted(),
            if configured {
                format!(
                    "{} · {}",
                    state.prefs.assistant.provider,
                    state.prefs.assistant.model.trim()
                )
            } else {
                "No model set — Preferences › General › Assistant".to_owned()
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if state.console.busy {
                if ui.small_button("Stop").clicked() {
                    state.console.stop_requested = true;
                }
            } else if !state.console.transcript.is_empty() && ui.small_button("Clear").clicked() {
                state.console.clear();
            }
        });
    });

    // A fixed height for the transcript, not "what is left": the rail hands
    // a panel as much height as it asks for, so what is left is endless and
    // the input line would sit below the window. Twenty-odd lines, then it
    // scrolls; the rail scrolls the rest.
    let scroll_to_end = std::mem::take(&mut state.console.scroll_to_end);
    egui::ScrollArea::vertical()
        .id_salt("console-transcript")
        .max_height(TRANSCRIPT_HEIGHT)
        .min_scrolled_height(60.0)
        .auto_shrink([false, true])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            for line in &state.console.transcript {
                match line {
                    Line::You(text) => {
                        ui.add_space(Theme::space_1());
                        ui.label(egui::RichText::new(text).strong());
                    }
                    Line::Model(text) => {
                        ui.label(text);
                    }
                    Line::Tool {
                        name,
                        summary,
                        is_error,
                    } => {
                        let colour = if *is_error {
                            Theme::error()
                        } else {
                            Theme::text_muted()
                        };
                        ui.colored_label(colour, format!("{name} → {summary}"));
                    }
                    Line::Error(text) => {
                        ui.colored_label(Theme::error(), text);
                    }
                    Line::Note(text) => {
                        ui.colored_label(Theme::text_muted(), text);
                    }
                }
            }
            if state.console.busy {
                ui.colored_label(Theme::text_muted(), "…");
            }
            if scroll_to_end {
                ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
            }
        });

    ui.add_space(Theme::space_1());
    let response = ui.add_enabled(
        !state.console.busy && configured,
        egui::TextEdit::singleline(&mut state.console.input)
            .desired_width(f32::INFINITY)
            .hint_text(if configured {
                "Ask the model to do something on the page"
            } else {
                "Set a model in Preferences first"
            }),
    );
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        state.console.send();
        response.request_focus();
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
}
