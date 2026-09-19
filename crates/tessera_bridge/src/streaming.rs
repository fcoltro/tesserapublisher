//! A reply as it arrives, word by word.
//!
//! Both providers can send a reply as server-sent events: a text stream of
//! `data:` lines, each a JSON piece of the reply, ending on a blank line.
//! Reading it as it comes means the transcript fills while the model is
//! still talking — the difference between a console and a form that hangs
//! for ten seconds and then speaks — and a Stop that lands mid-sentence
//! rather than after it.
//!
//! What comes out the far end is the same [`Step`] the whole-body path
//! produces, built from the pieces: text deltas joined, a tool call's
//! arguments assembled from JSON fragments. So everything after the round
//! trip — the tool loop, the transcript, the tests of both — is one path.
//! A transport that cannot stream says so, and the session posts whole.

use serde_json::Value;

use crate::assistant::{Step, ToolCall};

/// Server-sent events, cut out of a byte stream that arrives in pieces of
/// any size: a chunk may end mid-line, mid-event, mid-UTF-8 sequence.
#[derive(Default)]
pub struct Sse {
    pending: Vec<u8>,
}

impl Sse {
    /// Feed what arrived; get back the `data` of every event completed by
    /// it, each event's `data:` lines joined by newlines as the standard
    /// says. Comments, `event:` and `id:` lines are read past.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(chunk);
        let mut out = Vec::new();
        // An event ends at a blank line; only whole events are cut out, so
        // a chunk ending mid-line waits for the rest.
        while let Some(end) = find_blank_line(&self.pending) {
            let (event, rest_at) = end;
            let text = String::from_utf8_lossy(&self.pending[..event]).into_owned();
            self.pending.drain(..rest_at);
            let data: Vec<&str> = text
                .lines()
                .filter_map(|line| {
                    let line = line.strip_suffix('\r').unwrap_or(line);
                    line.strip_prefix("data:")
                        .map(|d| d.strip_prefix(' ').unwrap_or(d))
                })
                .collect();
            if !data.is_empty() {
                out.push(data.join("\n"));
            }
        }
        out
    }
}

/// Where the first blank line ends an event: `(event end, next start)`.
fn find_blank_line(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            // "\n\n" or "\n\r\n"
            if bytes.get(i + 1) == Some(&b'\n') {
                return Some((i, i + 2));
            }
            if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                return Some((i, i + 3));
            }
        }
        i += 1;
    }
    None
}

/// Builds a [`Step`] out of one provider's stream of events.
pub trait Accumulate {
    /// One event's data. Text the model has said so far is handed to
    /// `said` as it arrives.
    fn feed(&mut self, data: &str, said: &mut dyn FnMut(&str)) -> Result<(), String>;
    /// The stream is over: what it amounted to.
    fn finish(self: Box<Self>) -> Result<Step, String>;
}

/// The Messages API's stream: blocks start, take deltas, and stop; a tool
/// call's `input` arrives as fragments of JSON to be joined.
#[derive(Default)]
pub struct Anthropic {
    text: String,
    /// `(id, name, json so far)` per tool_use block, by block index.
    calls: Vec<(usize, String, String, String)>,
    error: Option<String>,
}

impl Accumulate for Anthropic {
    fn feed(&mut self, data: &str, said: &mut dyn FnMut(&str)) -> Result<(), String> {
        let event: Value = serde_json::from_str(data)
            .map_err(|e| format!("a stream event was not JSON: {e}: {}", excerpt(data)))?;
        match event["type"].as_str().unwrap_or("") {
            "content_block_start" => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                let block = &event["content_block"];
                if block["type"] == "tool_use" {
                    self.calls.push((
                        index,
                        block["id"].as_str().unwrap_or("").to_owned(),
                        block["name"].as_str().unwrap_or("").to_owned(),
                        String::new(),
                    ));
                } else if let Some(text) = block["text"].as_str()
                    && !text.is_empty()
                {
                    self.text.push_str(text);
                    said(text);
                }
            }
            "content_block_delta" => {
                let index = event["index"].as_u64().unwrap_or(0) as usize;
                let delta = &event["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let text = delta["text"].as_str().unwrap_or("");
                        self.text.push_str(text);
                        said(text);
                    }
                    "input_json_delta" => {
                        if let Some(call) = self.calls.iter_mut().find(|c| c.0 == index) {
                            call.3
                                .push_str(delta["partial_json"].as_str().unwrap_or(""));
                        }
                    }
                    _ => {}
                }
            }
            "error" => {
                self.error = Some(
                    event["error"]["message"]
                        .as_str()
                        .unwrap_or("unknown error")
                        .to_owned(),
                );
            }
            // ping, message_start, message_delta, content_block_stop,
            // message_stop: nothing to keep.
            _ => {}
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Step, String> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let calls: Vec<ToolCall> = self
            .calls
            .into_iter()
            .map(|(_, id, name, json)| {
                let arguments = if json.trim().is_empty() {
                    Value::Object(Default::default())
                } else {
                    serde_json::from_str(&json).map_err(|e| {
                        format!("tool arguments were not JSON: {e}: {}", excerpt(&json))
                    })?
                };
                Ok(ToolCall {
                    id,
                    name,
                    arguments,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(if calls.is_empty() {
            Step::Reply(self.text)
        } else {
            Step::Calls {
                text: self.text,
                calls,
            }
        })
    }
}

/// The chat-completions stream: each chunk a delta of the message, a tool
/// call's arguments arriving as string fragments by index, `[DONE]` last.
#[derive(Default)]
pub struct OpenAi {
    text: String,
    /// `(index, id, name, arguments so far)`.
    calls: Vec<(usize, String, String, String)>,
    error: Option<String>,
}

impl Accumulate for OpenAi {
    fn feed(&mut self, data: &str, said: &mut dyn FnMut(&str)) -> Result<(), String> {
        if data.trim() == "[DONE]" {
            return Ok(());
        }
        let event: Value = serde_json::from_str(data)
            .map_err(|e| format!("a stream event was not JSON: {e}: {}", excerpt(data)))?;
        if let Some(error) = event.get("error") {
            self.error = Some(
                error["message"]
                    .as_str()
                    .unwrap_or("unknown error")
                    .to_owned(),
            );
            return Ok(());
        }
        let delta = &event["choices"][0]["delta"];
        if let Some(text) = delta["content"].as_str()
            && !text.is_empty()
        {
            self.text.push_str(text);
            said(text);
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for piece in calls {
                let index = piece["index"].as_u64().unwrap_or(0) as usize;
                let slot = match self.calls.iter_mut().find(|c| c.0 == index) {
                    Some(slot) => slot,
                    None => {
                        self.calls
                            .push((index, String::new(), String::new(), String::new()));
                        self.calls.last_mut().expect("just pushed")
                    }
                };
                if let Some(id) = piece["id"].as_str() {
                    slot.1 = id.to_owned();
                }
                if let Some(name) = piece["function"]["name"].as_str() {
                    slot.2.push_str(name);
                }
                if let Some(arguments) = piece["function"]["arguments"].as_str() {
                    slot.3.push_str(arguments);
                }
            }
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Step, String> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let mut calls = self.calls;
        calls.sort_by_key(|c| c.0);
        let calls: Vec<ToolCall> = calls
            .into_iter()
            .map(|(_, id, name, json)| {
                let arguments = if json.trim().is_empty() {
                    Value::Object(Default::default())
                } else {
                    serde_json::from_str(&json).map_err(|e| {
                        format!("tool arguments were not JSON: {e}: {}", excerpt(&json))
                    })?
                };
                Ok(ToolCall {
                    id,
                    name,
                    arguments,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(if calls.is_empty() {
            Step::Reply(self.text)
        } else {
            Step::Calls {
                text: self.text,
                calls,
            }
        })
    }
}

fn excerpt(s: &str) -> String {
    s.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_cut_whole_from_pieces_of_any_size() {
        let stream = "event: message_start\ndata: {\"a\":1}\n\ndata: {\"b\":2}\ndata: {\"c\":3}\n\n: keepalive\n\ndata: [DONE]\n\n";
        // Every way of cutting it into two, three and single bytes gives
        // the same four events.
        for size in [1usize, 2, 3, 7, 100] {
            let mut sse = Sse::default();
            let mut got = Vec::new();
            for chunk in stream.as_bytes().chunks(size) {
                got.extend(sse.push(chunk));
            }
            assert_eq!(
                got,
                vec!["{\"a\":1}", "{\"b\":2}\n{\"c\":3}", "[DONE]"],
                "cut every {size} bytes"
            );
        }
        // CRLF line ends, as some proxies rewrite them.
        let mut sse = Sse::default();
        assert_eq!(sse.push(b"data: x\r\n\r\ndata: y\r\n\r\n"), vec!["x", "y"]);
    }

    #[test]
    fn an_anthropic_stream_builds_the_words_and_the_calls() {
        let events = [
            r#"{"type":"message_start","message":{"id":"m1"}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Adding "}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"it."}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"add_rectangle","input":{}}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"x\":0,"}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"y\":5}"}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
            r#"{"type":"message_stop"}"#,
        ];
        let mut acc = Box::new(Anthropic::default());
        let mut said = String::new();
        for e in events {
            acc.feed(e, &mut |t| said.push_str(t)).unwrap();
        }
        assert_eq!(said, "Adding it.", "the words came as they were said");
        let step = acc.finish().unwrap();
        assert_eq!(
            step,
            Step::Calls {
                text: "Adding it.".into(),
                calls: vec![ToolCall {
                    id: "t1".into(),
                    name: "add_rectangle".into(),
                    arguments: serde_json::json!({ "x": 0, "y": 5 }),
                }],
            }
        );

        // Words alone end the turn; an error event is the error.
        let mut acc = Box::new(Anthropic::default());
        acc.feed(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Done."}}"#,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(acc.finish().unwrap(), Step::Reply("Done.".into()));
        let mut acc = Box::new(Anthropic::default());
        acc.feed(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(acc.finish().unwrap_err(), "Overloaded");
    }

    #[test]
    fn a_chat_completions_stream_builds_the_words_and_the_calls() {
        let events = [
            r#"{"choices":[{"delta":{"role":"assistant","content":""}}]}"#,
            r#"{"choices":[{"delta":{"content":"One "}}]}"#,
            r#"{"choices":[{"delta":{"content":"page."}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"describe_","arguments":""}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"document","arguments":"{\"de"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"tail\":true}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "[DONE]",
        ];
        let mut acc = Box::new(OpenAi::default());
        let mut said = String::new();
        for e in events {
            acc.feed(e, &mut |t| said.push_str(t)).unwrap();
        }
        assert_eq!(said, "One page.");
        assert_eq!(
            acc.finish().unwrap(),
            Step::Calls {
                text: "One page.".into(),
                calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "describe_document".into(),
                    arguments: serde_json::json!({ "detail": true }),
                }],
            }
        );
    }
}
