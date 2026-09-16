//! A model at the console: the loop between a person's words and the tools.
//!
//! The person types; the model is sent the conversation and the tools;
//! the model answers with words, or with tool calls; each call is run and
//! its result sent back; the model answers again — until it has only words
//! left, which are shown. That loop is here, provider-neutral, and tested
//! against a transport that returns canned replies. The HTTP is not: a
//! [`Transport`] is handed in, the way the update check is handed its
//! fetch, so this crate needs no HTTP client and the loop's decisions have
//! tests.
//!
//! Two providers cover nearly every model a person can get a key for:
//! Anthropic's Messages API, and the OpenAI chat-completions shape, which
//! is also what Ollama, Groq, Mistral, OpenRouter, DeepSeek, LM Studio and
//! Gemini's compatible endpoint serve. Each is one adapter from the neutral
//! [`Message`] to its JSON and back.

use serde_json::{Value, json};

/// Something that can POST a JSON body and return the JSON reply.
pub trait Transport: Send {
    fn post(&self, url: &str, headers: &[(String, String)], body: &str) -> Result<String, String>;
}

/// Where the model is, and how to be let in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    Anthropic {
        api_key: String,
        model: String,
        /// `https://api.anthropic.com` unless a proxy says otherwise.
        base_url: String,
    },
    /// The OpenAI chat-completions shape, wherever it is served.
    OpenAiCompatible {
        /// Empty for a local server that asks for none.
        api_key: String,
        model: String,
        /// Up to and including `/v1`, e.g. `http://localhost:11434/v1`.
        base_url: String,
    },
}

impl Provider {
    /// The provider named in the preferences, or none while they are blank.
    pub fn from_settings(kind: &str, api_key: &str, model: &str, base_url: &str) -> Option<Self> {
        let model = model.trim();
        if model.is_empty() {
            return None;
        }
        match kind {
            "anthropic" => {
                if api_key.trim().is_empty() {
                    return None;
                }
                Some(Provider::Anthropic {
                    api_key: api_key.trim().to_owned(),
                    model: model.to_owned(),
                    base_url: if base_url.trim().is_empty() {
                        "https://api.anthropic.com".to_owned()
                    } else {
                        base_url.trim().trim_end_matches('/').to_owned()
                    },
                })
            }
            "openai" => Some(Provider::OpenAiCompatible {
                api_key: api_key.trim().to_owned(),
                model: model.to_owned(),
                base_url: if base_url.trim().is_empty() {
                    "https://api.openai.com/v1".to_owned()
                } else {
                    base_url.trim().trim_end_matches('/').to_owned()
                },
            }),
            _ => None,
        }
    }

    pub fn model(&self) -> &str {
        match self {
            Provider::Anthropic { model, .. } | Provider::OpenAiCompatible { model, .. } => model,
        }
    }
}

/// One tool call the model asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// The conversation, in neither provider's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    User(String),
    Assistant {
        text: String,
        calls: Vec<ToolCall>,
    },
    ToolResult {
        id: String,
        name: String,
        content: String,
        is_error: bool,
    },
}

/// What one round trip to the model came back as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Words, and the turn is over.
    Reply(String),
    /// Tools to run; their results go back and the model is asked again.
    Calls { text: String, calls: Vec<ToolCall> },
}

/// One conversation with one model.
pub struct Session {
    pub provider: Provider,
    pub system: String,
    pub tools: Vec<Value>,
    pub messages: Vec<Message>,
    transport: Box<dyn Transport>,
}

/// How many round trips one turn may take before it is stopped: a model
/// that calls tools forever is a bill, not a layout.
pub const MAX_STEPS: usize = 40;

impl Session {
    pub fn new(
        provider: Provider,
        system: String,
        tools: Vec<Value>,
        transport: Box<dyn Transport>,
    ) -> Self {
        Self {
            provider,
            system,
            tools,
            messages: Vec::new(),
            transport,
        }
    }

    /// What the person said.
    pub fn ask(&mut self, text: &str) {
        self.messages.push(Message::User(text.to_owned()));
    }

    /// What a tool answered.
    pub fn answer(&mut self, call: &ToolCall, content: String, is_error: bool) {
        self.messages.push(Message::ToolResult {
            id: call.id.clone(),
            name: call.name.clone(),
            content,
            is_error,
        });
    }

    /// One round trip: send everything so far, read the reply, remember it.
    pub fn step(&mut self) -> Result<Step, String> {
        let (url, headers, body) = match &self.provider {
            Provider::Anthropic { .. } => anthropic::request(self),
            Provider::OpenAiCompatible { .. } => openai::request(self),
        };
        let reply = self.transport.post(&url, &headers, &body.to_string())?;
        let reply: Value = serde_json::from_str(&reply)
            .map_err(|e| format!("the model's reply was not JSON: {e}: {}", excerpt(&reply)))?;
        if let Some(error) = reply.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!("{}: {message}", self.provider.model()));
        }
        let step = match &self.provider {
            Provider::Anthropic { .. } => anthropic::reply(&reply)?,
            Provider::OpenAiCompatible { .. } => openai::reply(&reply)?,
        };
        match &step {
            Step::Reply(text) => self.messages.push(Message::Assistant {
                text: text.clone(),
                calls: Vec::new(),
            }),
            Step::Calls { text, calls } => self.messages.push(Message::Assistant {
                text: text.clone(),
                calls: calls.clone(),
            }),
        }
        Ok(step)
    }

    /// Run a whole turn: ask, then step and run tools until the model has
    /// only words left. `run` executes one tool call and says what it
    /// returned and whether that was an error; `heard` is told each thing
    /// as it happens, for a transcript.
    pub fn turn(
        &mut self,
        text: &str,
        mut run: impl FnMut(&ToolCall) -> (String, bool),
        mut heard: impl FnMut(&Event),
    ) -> Result<String, String> {
        self.ask(text);
        for _ in 0..MAX_STEPS {
            match self.step()? {
                Step::Reply(reply) => {
                    heard(&Event::Said(reply.clone()));
                    return Ok(reply);
                }
                Step::Calls { text, calls } => {
                    if !text.is_empty() {
                        heard(&Event::Said(text));
                    }
                    for call in &calls {
                        let (content, is_error) = run(call);
                        heard(&Event::Ran {
                            call: call.clone(),
                            result: content.clone(),
                            is_error,
                        });
                        self.answer(call, content, is_error);
                    }
                }
            }
        }
        Err(format!(
            "stopped after {MAX_STEPS} tool calls in one turn; ask again to continue"
        ))
    }
}

/// Something that happened during a turn, for whoever is watching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Said(String),
    Ran {
        call: ToolCall,
        result: String,
        is_error: bool,
    },
}

fn excerpt(s: &str) -> String {
    s.chars().take(200).collect()
}

mod anthropic {
    use super::*;

    pub fn request(session: &Session) -> (String, Vec<(String, String)>, Value) {
        let Provider::Anthropic {
            api_key,
            model,
            base_url,
        } = &session.provider
        else {
            unreachable!("anthropic request for another provider")
        };
        let tools: Vec<Value> = session
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t["name"],
                    "description": t["description"],
                    "input_schema": t["inputSchema"],
                })
            })
            .collect();
        let mut messages: Vec<Value> = Vec::new();
        for m in &session.messages {
            match m {
                Message::User(text) => messages.push(json!({ "role": "user", "content": text })),
                Message::Assistant { text, calls } => {
                    let mut content = Vec::new();
                    if !text.is_empty() {
                        content.push(json!({ "type": "text", "text": text }));
                    }
                    for c in calls {
                        content.push(json!({
                            "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments,
                        }));
                    }
                    if content.is_empty() {
                        content.push(json!({ "type": "text", "text": "…" }));
                    }
                    messages.push(json!({ "role": "assistant", "content": content }));
                }
                Message::ToolResult {
                    id,
                    content,
                    is_error,
                    ..
                } => {
                    // Consecutive results share one user message, as the
                    // API asks.
                    let block = json!({
                        "type": "tool_result", "tool_use_id": id, "content": content,
                        "is_error": is_error,
                    });
                    match messages.last_mut() {
                        Some(last)
                            if last["role"] == "user"
                                && last["content"].is_array()
                                && last["content"][0]["type"] == "tool_result" =>
                        {
                            last["content"].as_array_mut().unwrap().push(block);
                        }
                        _ => messages.push(json!({ "role": "user", "content": [block] })),
                    }
                }
            }
        }
        let body = json!({
            "model": model,
            "max_tokens": 4096,
            "system": session.system,
            "tools": tools,
            "messages": messages,
        });
        let headers = vec![
            ("x-api-key".to_owned(), api_key.clone()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
            ("content-type".to_owned(), "application/json".to_owned()),
        ];
        (format!("{base_url}/v1/messages"), headers, body)
    }

    pub fn reply(reply: &Value) -> Result<Step, String> {
        let blocks = reply
            .get("content")
            .and_then(Value::as_array)
            .ok_or("the reply had no content")?;
        let mut text = String::new();
        let mut calls = Vec::new();
        for b in blocks {
            match b.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }
                Some("tool_use") => calls.push(ToolCall {
                    id: b["id"].as_str().unwrap_or("").to_owned(),
                    name: b["name"].as_str().unwrap_or("").to_owned(),
                    arguments: b.get("input").cloned().unwrap_or(json!({})),
                }),
                _ => {}
            }
        }
        if calls.is_empty() {
            Ok(Step::Reply(text))
        } else {
            Ok(Step::Calls { text, calls })
        }
    }
}

mod openai {
    use super::*;

    pub fn request(session: &Session) -> (String, Vec<(String, String)>, Value) {
        let Provider::OpenAiCompatible {
            api_key,
            model,
            base_url,
        } = &session.provider
        else {
            unreachable!("openai request for another provider")
        };
        let tools: Vec<Value> = session
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t["name"],
                        "description": t["description"],
                        "parameters": t["inputSchema"],
                    },
                })
            })
            .collect();
        let mut messages = vec![json!({ "role": "system", "content": session.system })];
        for m in &session.messages {
            match m {
                Message::User(text) => messages.push(json!({ "role": "user", "content": text })),
                Message::Assistant { text, calls } => {
                    let mut msg = json!({ "role": "assistant", "content": text });
                    if !calls.is_empty() {
                        msg["tool_calls"] = json!(
                            calls
                                .iter()
                                .map(|c| json!({
                                    "id": c.id,
                                    "type": "function",
                                    "function": {
                                        "name": c.name,
                                        "arguments": c.arguments.to_string(),
                                    },
                                }))
                                .collect::<Vec<_>>()
                        );
                    }
                    messages.push(msg);
                }
                Message::ToolResult { id, content, .. } => {
                    messages
                        .push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
                }
            }
        }
        let body = json!({ "model": model, "messages": messages, "tools": tools });
        let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
        if !api_key.is_empty() {
            headers.push(("authorization".to_owned(), format!("Bearer {api_key}")));
        }
        (format!("{base_url}/chat/completions"), headers, body)
    }

    pub fn reply(reply: &Value) -> Result<Step, String> {
        let message = reply
            .pointer("/choices/0/message")
            .ok_or("the reply had no choices")?;
        let text = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let calls: Vec<ToolCall> = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|calls| {
                calls
                    .iter()
                    .map(|c| {
                        let arguments = c
                            .pointer("/function/arguments")
                            .and_then(Value::as_str)
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or(json!({}));
                        ToolCall {
                            id: c["id"].as_str().unwrap_or("").to_owned(),
                            name: c
                                .pointer("/function/name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_owned(),
                            arguments,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if calls.is_empty() {
            Ok(Step::Reply(text))
        } else {
            Ok(Step::Calls { text, calls })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex};

    /// Canned replies, in order, remembering what was asked.
    struct Canned {
        replies: Mutex<Vec<String>>,
        asked: Arc<Mutex<Vec<(String, Vec<(String, String)>, Value)>>>,
    }

    impl Transport for Canned {
        fn post(
            &self,
            url: &str,
            headers: &[(String, String)],
            body: &str,
        ) -> Result<String, String> {
            self.asked.lock().unwrap().push((
                url.to_owned(),
                headers.to_vec(),
                serde_json::from_str(body).unwrap(),
            ));
            let mut replies = self.replies.lock().unwrap();
            if replies.is_empty() {
                return Err("no more canned replies".into());
            }
            Ok(replies.remove(0))
        }
    }

    fn canned(
        replies: &[Value],
    ) -> (
        Box<Canned>,
        Arc<Mutex<Vec<(String, Vec<(String, String)>, Value)>>>,
    ) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        (
            Box::new(Canned {
                replies: Mutex::new(replies.iter().map(Value::to_string).collect()),
                asked: asked.clone(),
            }),
            asked,
        )
    }

    fn tools() -> Vec<Value> {
        crate::tools::list()
    }

    #[test]
    fn an_anthropic_turn_runs_the_tools_it_is_asked_for_and_ends_in_words() {
        let (transport, asked) = canned(&[
            json!({ "content": [
                { "type": "text", "text": "Adding it." },
                { "type": "tool_use", "id": "t1", "name": "add_rectangle",
                  "input": { "x": 0, "y": 0, "width": 10, "height": 10 } },
            ], "stop_reason": "tool_use" }),
            json!({ "content": [{ "type": "text", "text": "Done: one rectangle." }], "stop_reason": "end_turn" }),
        ]);
        let provider = Provider::from_settings("anthropic", "sk-test", "claude-x", "").unwrap();
        let mut session = Session::new(provider, "You lay out pages.".into(), tools(), transport);

        let state = RefCell::new(tessera_ui::TesseraApp::headless());
        let events = RefCell::new(Vec::new());
        let reply = session
            .turn(
                "Add a rectangle",
                |call| {
                    let mut state = state.borrow_mut();
                    match crate::tools::call(&mut state, &call.name, &call.arguments) {
                        Ok(v) => (v.to_string(), false),
                        Err(crate::Failure::Refused(m)) => (m, true),
                        Err(crate::Failure::NoSuchTool) => ("no such tool".into(), true),
                    }
                },
                |e| events.borrow_mut().push(e.clone()),
            )
            .unwrap();
        assert_eq!(reply, "Done: one rectangle.");
        assert_eq!(state.borrow().active().document().frames.len(), 1);
        let events = events.borrow();
        assert!(matches!(&events[0], Event::Said(t) if t == "Adding it."));
        assert!(
            matches!(&events[1], Event::Ran { call, is_error: false, .. } if call.name == "add_rectangle")
        );

        // What was sent: the right URL and headers, the tools, and on the
        // second call the tool result in a user message.
        let asked = asked.lock().unwrap();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0].0, "https://api.anthropic.com/v1/messages");
        assert!(
            asked[0]
                .1
                .iter()
                .any(|(k, v)| k == "x-api-key" && v == "sk-test")
        );
        assert_eq!(asked[0].2["system"], "You lay out pages.");
        assert!(asked[0].2["tools"].as_array().unwrap().len() > 40);
        assert_eq!(asked[0].2["tools"][0]["input_schema"]["type"], "object");
        let second = &asked[1].2["messages"];
        assert_eq!(second[1]["role"], "assistant");
        assert_eq!(second[1]["content"][1]["type"], "tool_use");
        assert_eq!(second[2]["role"], "user");
        assert_eq!(second[2]["content"][0]["type"], "tool_result");
        assert_eq!(second[2]["content"][0]["tool_use_id"], "t1");
        assert_eq!(second[2]["content"][0]["is_error"], false);
    }

    #[test]
    fn an_openai_compatible_turn_speaks_that_shape() {
        let (transport, asked) = canned(&[
            json!({ "choices": [{ "message": { "role": "assistant", "content": null,
                "tool_calls": [{ "id": "c1", "type": "function", "function": {
                    "name": "describe_document", "arguments": "{}" } }] }, "finish_reason": "tool_calls" }] }),
            json!({ "choices": [{ "message": { "role": "assistant", "content": "One page, no frames." }, "finish_reason": "stop" }] }),
        ]);
        let provider =
            Provider::from_settings("openai", "", "llama3", "http://localhost:11434/v1/").unwrap();
        let mut session = Session::new(provider, "sys".into(), tools(), transport);
        let reply = session
            .turn(
                "What is on the page?",
                |_| ("{\"pages\":1}".into(), false),
                |_| {},
            )
            .unwrap();
        assert_eq!(reply, "One page, no frames.");
        let asked = asked.lock().unwrap();
        assert_eq!(asked[0].0, "http://localhost:11434/v1/chat/completions");
        assert!(
            !asked[0].1.iter().any(|(k, _)| k == "authorization"),
            "no key, no header"
        );
        assert_eq!(asked[0].2["tools"][0]["type"], "function");
        assert_eq!(asked[0].2["messages"][0]["role"], "system");
        let second = &asked[1].2["messages"];
        assert_eq!(
            second[2]["tool_calls"][0]["function"]["name"],
            "describe_document"
        );
        assert_eq!(second[3]["role"], "tool");
        assert_eq!(second[3]["tool_call_id"], "c1");
    }

    #[test]
    fn a_provider_error_is_the_model_s_words_and_a_runaway_turn_is_stopped() {
        let (transport, _) = canned(&[
            json!({ "error": { "type": "authentication_error", "message": "invalid x-api-key" } }),
        ]);
        let provider = Provider::from_settings("anthropic", "bad", "claude-x", "").unwrap();
        let mut session = Session::new(provider, "sys".into(), tools(), transport);
        let err = session
            .turn("hi", |_| ("".into(), false), |_| {})
            .unwrap_err();
        assert!(err.contains("invalid x-api-key"), "{err}");

        // A model that never stops calling tools is stopped.
        let forever: Vec<Value> = (0..MAX_STEPS + 1)
            .map(|_| json!({ "content": [{ "type": "tool_use", "id": "t", "name": "undo", "input": {} }] }))
            .collect();
        let (transport, _) = canned(&forever);
        let provider = Provider::from_settings("anthropic", "k", "claude-x", "").unwrap();
        let mut session = Session::new(provider, "sys".into(), tools(), transport);
        let err = session
            .turn("loop", |_| ("ok".into(), false), |_| {})
            .unwrap_err();
        assert!(err.contains("stopped after"), "{err}");
    }

    #[test]
    fn settings_make_a_provider_or_none_while_blank() {
        assert!(
            Provider::from_settings("anthropic", "", "claude-x", "").is_none(),
            "no key"
        );
        assert!(
            Provider::from_settings("anthropic", "k", "", "").is_none(),
            "no model"
        );
        assert!(
            Provider::from_settings("openai", "", "llama3", "").is_some(),
            "a local server needs no key"
        );
        assert!(Provider::from_settings("bard", "k", "m", "").is_none());
        match Provider::from_settings("openai", "k", "gpt", "").unwrap() {
            Provider::OpenAiCompatible { base_url, .. } => {
                assert_eq!(base_url, "https://api.openai.com/v1")
            }
            other => panic!("{other:?}"),
        }
    }
}
