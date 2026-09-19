//! Driving the console: the model on a thread, the tools on the UI thread.
//!
//! The panel in `tessera_ui` holds the words and knows no model. This takes
//! the prompts it collects, runs each turn on a thread of its own — the
//! round trips to a provider take seconds, and a frame loop waiting on them
//! would be a frozen window — and puts what happened back into the
//! transcript. A tool the model asks for cannot run on that thread, because
//! the document belongs to the UI thread: it crosses on a channel with a
//! reply channel of its own, the frame loop answers it in [`Driver::pump`],
//! and the turn goes on. The same arrangement as the socket in
//! [`crate::live`], for the same reason.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use tessera_ui::TesseraApp;
use tessera_ui::view::console::{Line, context_line};

use crate::assistant::{Event, Image, Outcome, Provider, Session, ToolCall, Transport};

/// Makes a transport for each session: the binary hands in a ureq one, a
/// test hands in canned replies.
pub type MakeTransport = dyn Fn() -> Box<dyn Transport> + Send + Sync;

/// What the model is told once, at the start of a session. The tools'
/// own descriptions carry the rest.
pub fn system_prompt() -> String {
    format!(
        "{}\n\nYou are working at the console inside Tessera, and the person is watching \
         the canvas as you work. Each prompt begins with a bracketed context line saying \
         what is open and selected; trust it over memory. Read describe_document before \
         placing anything, and place inside the page's bounds. Prefer the named tools; \
         reach for `command` when no named tool does the thing. Keep replies to a sentence \
         or two — the work shows on the page, not in the transcript. render_page lets you \
         look at what you made.",
        crate::tools::INSTRUCTIONS
    )
}

/// The picture a tool's result points at, read for the model: a
/// `rendered` PNG path — what `render_page` returns — becomes the image
/// itself, so a model with eyes looks at the page rather than at a path
/// it has no way to open. Capped, because a page at 600 ppi is a request
/// nobody's context window wants: past four megabytes the path stands.
pub(crate) fn picture_in(value: &serde_json::Value) -> Option<Image> {
    const LARGEST: u64 = 4 * 1024 * 1024;
    let path = value.get("rendered")?.as_str()?;
    if !path.to_ascii_lowercase().ends_with(".png") {
        return None;
    }
    let size = std::fs::metadata(path).ok()?.len();
    if size > LARGEST {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(Image::png(&bytes))
}

/// A tool call waiting for the UI thread, and where to send its result.
struct Pending {
    call: ToolCall,
    reply: Sender<Outcome>,
}

/// What a turn's thread sends back as it goes.
enum Happened {
    Event(Event),
    Finished(Result<String, String>, Box<Session>),
}

/// A turn under way.
struct Running {
    document: tessera_ui::app::DocumentKey,
    happened: Receiver<Happened>,
    pending: Receiver<Pending>,
    cancel: Arc<AtomicBool>,
    /// The transcript line the model's words are arriving into, while
    /// they are.
    saying: Option<usize>,
}

pub struct Driver {
    make_transport: Arc<MakeTransport>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The conversation so far, between turns. `None` before the first and
    /// after Clear.
    session: Option<Box<Session>>,
    /// Which provider the session was made for, so a change of model in
    /// the preferences starts a fresh one.
    session_provider: Option<Provider>,
    running: Option<Running>,
}

impl Driver {
    pub fn new(make_transport: Arc<MakeTransport>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            make_transport,
            wake,
            session: None,
            session_provider: None,
            running: None,
        }
    }

    /// Whether a turn is under way.
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Once a frame: answer the tools a turn is waiting on, show what it
    /// said, and start the next prompt when the last turn is done.
    pub fn pump(&mut self, state: &mut TesseraApp) {
        if state.console.transcript.is_empty() {
            // Cleared: the model forgets too.
            self.session = None;
        }
        if let Some(running) = &mut self.running {
            if state.active != running.document && !running.cancel.load(Ordering::SeqCst) {
                running.cancel.store(true, Ordering::SeqCst);
                state.console.heard(Line::Note(
                    "Stopped because the active document changed. Send a new prompt to continue."
                        .into(),
                ));
            }
            if std::mem::take(&mut state.console.stop_requested) {
                running.cancel.store(true, Ordering::SeqCst);
            }
            while let Ok(pending) = running.pending.try_recv() {
                let outcome = if running.cancel.load(Ordering::SeqCst) {
                    Outcome::text("stopped before execution", true)
                } else {
                    match crate::tools::call(state, &pending.call.name, &pending.call.arguments) {
                        Ok(value) => Outcome {
                            image: picture_in(&value),
                            content: value.to_string(),
                            is_error: false,
                        },
                        Err(crate::Failure::Refused(message)) => Outcome::text(message, true),
                        Err(crate::Failure::NoSuchTool) => {
                            Outcome::text(format!("no tool named {:?}", pending.call.name), true)
                        }
                    }
                };
                // A tool can deliberately open, close or switch documents.
                // Only that explicit action updates the turn's document identity.
                running.document = state.active;
                let _ = pending.reply.send(outcome);
            }
            let mut finished = None;
            while let Ok(happened) = running.happened.try_recv() {
                match happened {
                    // Words as they come go into a line that grows; the
                    // whole, when it comes, replaces it exactly.
                    Happened::Event(Event::Saying(piece)) => match running.saying {
                        Some(index) => {
                            if let Some(Line::Model(text)) = state.console.transcript.get_mut(index)
                            {
                                text.push_str(&piece);
                                state.console.touched();
                            }
                        }
                        None => {
                            state.console.heard(Line::Model(piece));
                            running.saying = Some(state.console.transcript.len() - 1);
                        }
                    },
                    Happened::Event(Event::Said(text)) => match running.saying.take() {
                        Some(index) => {
                            if let Some(Line::Model(shown)) =
                                state.console.transcript.get_mut(index)
                            {
                                *shown = text;
                            }
                        }
                        None => state.console.heard(Line::Model(text)),
                    },
                    Happened::Event(Event::Ran {
                        call,
                        result,
                        is_error,
                    }) => state.console.heard(Line::Tool {
                        name: call.name,
                        summary: summarise(&result, is_error),
                        is_error,
                    }),
                    Happened::Finished(outcome, session) => finished = Some((outcome, session)),
                }
            }
            if let Some((outcome, session)) = finished {
                if let Err(message) = outcome {
                    state.console.heard(Line::Error(message));
                }
                self.session = Some(session);
                self.running = None;
                state.console.busy = false;
            }
            return;
        }

        let Some(prompt) = state.console.take_prompt() else {
            return;
        };
        let settings = &state.prefs.assistant;
        let Some(provider) = Provider::from_settings(
            &settings.provider,
            &settings.api_key,
            &settings.model,
            &settings.base_url,
        ) else {
            state.console.heard(Line::Error(
                "No model is set. Preferences › General › Assistant: choose a provider, a model, \
                 and a key."
                    .into(),
            ));
            return;
        };
        if self.session_provider.as_ref() != Some(&provider) {
            self.session = None;
        }
        let session = self.session.take().unwrap_or_else(|| {
            Box::new(Session::new(
                provider.clone(),
                system_prompt(),
                crate::tools::list(),
                (self.make_transport)(),
            ))
        });
        self.session_provider = Some(provider);

        let text = format!("{}\n\n{prompt}", context_line(state));
        let (happened_tx, happened) = channel();
        let (pending_tx, pending) = channel::<Pending>();
        let cancel = Arc::new(AtomicBool::new(false));
        let wake = self.wake.clone();
        let cancel_for_thread = cancel.clone();
        std::thread::Builder::new()
            .name("tessera-console".into())
            .spawn(move || {
                let mut session = session;
                session.cancel = Some(cancel_for_thread);
                let run = |call: &ToolCall| -> Outcome {
                    let (reply_tx, reply_rx) = channel();
                    if pending_tx
                        .send(Pending {
                            call: call.clone(),
                            reply: reply_tx,
                        })
                        .is_err()
                    {
                        return Outcome::text("the window is gone", true);
                    }
                    wake();
                    reply_rx
                        .recv()
                        .unwrap_or_else(|_| Outcome::text("the window is gone", true))
                };
                let heard = |event: &Event| {
                    let _ = happened_tx.send(Happened::Event(event.clone()));
                    wake();
                };
                let outcome = session.turn(&text, run, heard);
                let _ = happened_tx.send(Happened::Finished(outcome, session));
                wake();
            })
            .ok();
        self.running = Some(Running {
            document: state.active,
            happened,
            pending,
            cancel,
            saying: None,
        });
        state.console.busy = true;
    }
}

/// A tool's result in a few words for the transcript: the frame it made,
/// the count it found, or the first line of what it said.
fn summarise(result: &str, is_error: bool) -> String {
    if is_error {
        return result
            .lines()
            .next()
            .unwrap_or(result)
            .chars()
            .take(120)
            .collect();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(result) else {
        return result.chars().take(80).collect();
    };
    let mut parts = Vec::new();
    for key in [
        "frame", "count", "pages", "replaced", "exported", "saved", "rendered", "ran",
    ] {
        if let Some(v) = value.get(key) {
            parts.push(format!("{key} {}", v.to_string().trim_matches('"')));
        }
    }
    if let Some(status) = value.get("status").and_then(|s| s.as_str()) {
        parts.push(status.to_owned());
    }
    if parts.is_empty() {
        "ok".to_owned()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    struct Canned(Mutex<Vec<String>>);
    impl Transport for Canned {
        fn post(&self, _: &str, _: &[(String, String)], _: &str) -> Result<String, String> {
            let mut replies = self.0.lock().unwrap();
            if replies.is_empty() {
                Err("no more replies".into())
            } else {
                Ok(replies.remove(0))
            }
        }
    }

    fn driver(replies: Vec<serde_json::Value>) -> Driver {
        let replies: Vec<String> = replies.iter().map(|v| v.to_string()).collect();
        let replies = Arc::new(Mutex::new(Some(replies)));
        Driver::new(
            Arc::new(move || {
                let taken = replies.lock().unwrap().take().unwrap_or_default();
                Box::new(Canned(Mutex::new(taken))) as Box<dyn Transport>
            }),
            Arc::new(|| {}),
        )
    }

    fn pump_until_idle(driver: &mut Driver, state: &mut TesseraApp) {
        let start = std::time::Instant::now();
        loop {
            driver.pump(state);
            if !driver.busy() && state.console.outbox.is_empty() {
                break;
            }
            assert!(start.elapsed().as_secs() < 10, "hung");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn a_prompt_runs_a_turn_whose_tools_land_in_the_document_and_transcript() {
        let mut state = TesseraApp::headless();
        state.prefs.assistant.provider = "openai".into();
        state.prefs.assistant.model = "test".into();
        let mut driver = driver(vec![
            json!({ "choices": [{ "message": { "role": "assistant", "content": "Making it.",
                "tool_calls": [{ "id": "c1", "type": "function", "function": {
                    "name": "add_rectangle", "arguments": "{\"x\":0,\"y\":0,\"width\":10,\"height\":10}" } }] } }] }),
            json!({ "choices": [{ "message": { "role": "assistant", "content": "There." } }] }),
        ]);
        state.console.input = "add a rectangle".into();
        state.console.outbox.push("add a rectangle".into());
        state.console.heard(Line::You("add a rectangle".into()));

        pump_until_idle(&mut driver, &mut state);

        assert_eq!(
            state.active().document().frames.len(),
            1,
            "the tool ran on the UI thread"
        );
        let lines = &state.console.transcript;
        assert!(
            matches!(&lines[1], Line::Model(t) if t == "Making it."),
            "{lines:?}"
        );
        assert!(
            matches!(&lines[2], Line::Tool { name, is_error: false, .. } if name == "add_rectangle"),
            "{lines:?}"
        );
        assert!(
            matches!(&lines[3], Line::Model(t) if t == "There."),
            "{lines:?}"
        );
        assert!(!state.console.busy);
        assert!(
            driver.session.is_some(),
            "the conversation is kept for the next turn"
        );
    }

    #[test]
    fn no_model_set_is_said_in_the_transcript_not_sent_anywhere() {
        let mut state = TesseraApp::headless();
        let mut driver = driver(vec![]);
        state.console.outbox.push("hello".into());
        state.console.heard(Line::You("hello".into()));
        pump_until_idle(&mut driver, &mut state);
        assert!(
            matches!(&state.console.transcript[1], Line::Error(m) if m.contains("Preferences"))
        );
        assert!(!driver.busy());
    }

    #[test]
    fn clearing_the_transcript_forgets_the_session() {
        let mut state = TesseraApp::headless();
        state.prefs.assistant.provider = "openai".into();
        state.prefs.assistant.model = "test".into();
        let mut driver = driver(vec![
            json!({ "choices": [{ "message": { "role": "assistant", "content": "Hi." } }] }),
        ]);
        state.console.outbox.push("hi".into());
        state.console.heard(Line::You("hi".into()));
        pump_until_idle(&mut driver, &mut state);
        assert!(driver.session.is_some());
        state.console.clear();
        driver.pump(&mut state);
        assert!(driver.session.is_none());
    }

    #[test]
    fn results_are_summarised_for_the_transcript() {
        assert_eq!(
            summarise(r#"{"frame":7,"revision":3,"status":null}"#, false),
            "frame 7"
        );
        assert_eq!(summarise(r#"{"count":2,"matches":[]}"#, false), "count 2");
        assert_eq!(
            summarise(r#"{"revision":3,"status":"Exported x.pdf"}"#, false),
            "Exported x.pdf"
        );
        assert_eq!(summarise(r#"{"revision":3,"status":null}"#, false), "ok");
        assert_eq!(
            summarise("no frame numbered 9", true),
            "no frame numbered 9"
        );
    }
}
